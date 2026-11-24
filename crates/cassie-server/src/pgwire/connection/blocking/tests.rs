//! Deferred cleanup stays with the actual worker, including unwinding and abort.

use super::*;
use crate::config::CassieRuntimeConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

fn runtime(blocking_threads: usize) -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(blocking_threads)
        .build()
        .expect("runtime")
}

fn fixture() -> Arc<Cassie> {
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture"),
    );
    cassie.startup().expect("startup");
    cassie
}

#[derive(Clone, Copy)]
enum Outcome {
    Success,
    Error,
    Panic,
}

#[test]
fn should_retire_worker_cleanup_off_the_reactor_for_every_outcome() {
    runtime(2).block_on(async {
        for outcome in [Outcome::Success, Outcome::Error, Outcome::Panic] {
            // Arrange
            let cassie = fixture();
            let control = cleanup_test_hooks::install(&cassie, false);
            let reactor = std::thread::current().id();
            let awaiting = tokio::spawn(run_pgwire_blocking(Arc::clone(&cassie), "owned_cleanup", move |_| {
                match outcome {
                    Outcome::Success => Ok(7),
                    Outcome::Error => Err(CassieError::Unsupported("original operation error".to_owned())),
                    Outcome::Panic => panic!("original operation panic"),
                }
            }));
            // Act
            let observed = tokio::time::timeout(Duration::from_secs(10), control.entered).await.expect("cleanup boundary").expect("observation");
            let peer = tokio::spawn(async { 11 }).await.expect("reactor remains responsive");
            let pending = !awaiting.is_finished();
            control.resume.send(()).expect("resume cleanup");
            tokio::time::timeout(Duration::from_secs(10), control.completed).await.expect("cleanup completion").expect("worker completed cleanup");
            let result = awaiting.await.expect("awaiting task");
            // Assert
            assert_ne!(observed.worker, reactor);
            assert!(observed.recovery_allowed);
            assert_eq!(peer, 11);
            assert!(pending);
            assert_eq!(control.calls.load(Ordering::Acquire), 1);
            match outcome {
                Outcome::Success => assert_eq!(result.expect("success"), 7),
                Outcome::Error => assert!(matches!(result, Err(CassieError::Unsupported(message)) if message == "original operation error")),
                Outcome::Panic => {
                    let error = result.expect_err("original panic mapped through join");
                    assert!(matches!(error, CassieError::StorageRetryable(_)));
                    assert!(error.to_string().contains("original operation panic"));
                }
            }
        }
    });
}

#[test]
fn should_skip_native_cleanup_when_storage_requires_recovery() {
    // Arrange
    runtime(1).block_on(async {
        let cassie = fixture();
        cassie.midge.latch_storage_recovery_required_for_test();
        let control = cleanup_test_hooks::install(&cassie, false);
        // Act
        let awaiting = tokio::spawn(run_pgwire_blocking(
            Arc::clone(&cassie),
            "recovery_cleanup",
            |_| Ok(()),
        ));
        let observed = tokio::time::timeout(Duration::from_secs(10), control.entered)
            .await
            .expect("recovery decision")
            .expect("observation");
        control.resume.send(()).expect("release decision");
        control.completed.await.expect("worker finished");
        awaiting
            .await
            .expect("awaiting task")
            .expect("original result");
        // Assert
        assert!(!observed.recovery_allowed);
        assert_eq!(control.calls.load(Ordering::Acquire), 0);
        assert!(cassie.check_storage_recovery_required().is_err());
    });
}

#[test]
fn should_finish_started_worker_cleanup_after_its_awaiting_task_is_aborted() {
    // Arrange
    runtime(1).block_on(async {
        let cassie = fixture();
        let weak = Arc::downgrade(&cassie);
        let control = cleanup_test_hooks::install(&cassie, false);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let awaiting = tokio::spawn(run_pgwire_blocking(
            Arc::clone(&cassie),
            "aborted_cleanup",
            move |_| {
                started_tx.send(()).expect("operation started");
                resume_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("resume operation");
                Ok(())
            },
        ));
        tokio::time::timeout(Duration::from_secs(10), started_rx)
            .await
            .expect("operation admission")
            .expect("started");
        // Act
        awaiting.abort();
        assert!(awaiting
            .await
            .expect_err("awaiting task canceled")
            .is_cancelled());
        drop(cassie);
        resume_tx.send(()).expect("finish started operation");
        let observed = tokio::time::timeout(Duration::from_secs(10), control.entered)
            .await
            .expect("cleanup survives abort")
            .expect("observation");
        let retained = weak.upgrade().is_some();
        control.resume.send(()).expect("finish retained cleanup");
        tokio::time::timeout(Duration::from_secs(10), control.completed)
            .await
            .expect("retained cleanup completion")
            .expect("finished");
        // Assert
        assert!(retained, "the same worker owns Cassie through cleanup");
        assert!(observed.recovery_allowed);
        assert_eq!(control.calls.load(Ordering::Acquire), 1);
    });
}

#[test]
fn should_leave_cleanup_debt_for_later_when_a_queued_worker_never_starts() {
    // Arrange
    runtime(1).block_on(async {
        // Complete startup and create real adapter cleanup debt before the
        // only blocking-pool slot is deliberately occupied.
        let cassie = fixture();
        let session = cassie.create_session("queued_cleanup", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE queued_cleanup_debt (id INT)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .defer_drop_collection("queued_cleanup_debt", cassie.runtime().schema_epoch())
            .unwrap();
        let debt = cassie.midge.pending_schema_cleanups().unwrap();
        assert_eq!(debt.len(), 1);
        let (occupied_tx, occupied_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let occupied = tokio::task::spawn_blocking(move || {
            occupied_tx.send(()).expect("blocking slot occupied");
            release_rx
                .recv_timeout(Duration::from_secs(10))
                .expect("release blocking slot");
        });
        occupied_rx.await.expect("slot occupied");
        let control = cleanup_test_hooks::install(&cassie, true);
        let ran = Arc::new(AtomicBool::new(false));
        let worker_ran = Arc::clone(&ran);
        // Act
        let awaiting = tokio::spawn(run_pgwire_blocking(
            Arc::clone(&cassie),
            "queued_cleanup",
            move |_| {
                worker_ran.store(true, Ordering::Release);
                Ok(())
            },
        ));
        tokio::time::timeout(Duration::from_secs(10), control.queued_aborted)
            .await
            .expect("queued abort requested")
            .expect("abort observation");
        release_tx.send(()).expect("release occupied slot");
        occupied.await.expect("slot owner");
        let result = awaiting.await.expect("queued boundary");
        // Assert
        assert!(matches!(result, Err(CassieError::StorageRetryable(_))));
        assert!(!ran.load(Ordering::Acquire));
        assert_eq!(control.calls.load(Ordering::Acquire), 0);
        assert_eq!(cassie.midge.pending_schema_cleanups().unwrap(), debt);
        // Retire the never-consumed probe before exercising real future cleanup.
        cleanup_test_hooks::retire(cleanup_test_hooks::key(&cassie));
        run_pgwire_blocking(Arc::clone(&cassie), "later_cleanup", |_| Ok(()))
            .await
            .expect("later owned worker");
        assert_eq!(
            cassie.midge.pending_schema_cleanups().unwrap().as_slice(),
            &debt[..0]
        );
    });
}

#[test]
fn should_map_join_failures_to_retryable_storage_errors() {
    // Arrange
    runtime(1).block_on(async {
        let cassie = fixture();
        // Act
        let error =
            run_pgwire_blocking(cassie, "pgwire_retryable", |_| -> Result<(), CassieError> {
                panic!("synthetic pgwire join failure");
            })
            .await
            .expect_err("panic should map to retryable storage");
        // Assert
        assert!(matches!(error, CassieError::StorageRetryable(_)));
        assert!(error
            .to_string()
            .contains("pgwire blocking boundary 'pgwire_retryable' failed"));
    });
}
