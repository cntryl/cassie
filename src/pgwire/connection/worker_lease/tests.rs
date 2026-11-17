//! Connection admission remains owned by actual workers through final cleanup.
use super::*;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use std::sync::mpsc;
use std::time::Duration;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
}

#[test]
fn should_retain_the_connection_slot_after_await_abort_until_cleanup_retires() {
    // Arrange
    runtime().block_on(async {
        let cassie =
            Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
        cassie.startup().unwrap();
        let admission = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = Arc::clone(&admission).acquire_owned().await.unwrap();
        let control = super::super::blocking::cleanup_test_hooks::install(&cassie, false);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let awaiting = tokio::spawn(with_connection_lease(
            permit,
            super::super::blocking::run_pgwire_blocking(
                Arc::clone(&cassie),
                "retained_connection",
                move |_| {
                    started_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                    Ok(())
                },
            ),
        ));
        tokio::time::timeout(Duration::from_secs(10), started_rx)
            .await
            .unwrap()
            .unwrap();

        // Act
        awaiting.abort();
        assert!(awaiting.await.unwrap_err().is_cancelled());
        let held_during_operation = admission.try_acquire().is_err();
        resume_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(10), control.entered)
            .await
            .unwrap()
            .unwrap();
        let held_during_cleanup = admission.try_acquire().is_err();
        control.resume.send(()).unwrap();
        let released = tokio::time::timeout(
            Duration::from_secs(10),
            Arc::clone(&admission).acquire_owned(),
        )
        .await
        .unwrap()
        .unwrap();

        // Assert
        assert!(held_during_operation);
        assert!(held_during_cleanup);
        assert_eq!(released.num_permits(), 1);
        control.completed.await.unwrap();
    });
}

#[test]
fn should_release_a_never_started_workers_connection_lease_without_running_it() {
    // Arrange
    runtime().block_on(async {
        let (occupied_tx, occupied_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let occupied = tokio::task::spawn_blocking(move || {
            occupied_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        occupied_rx.await.unwrap();
        let admission = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = Arc::clone(&admission).acquire_owned().await.unwrap();
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = Arc::clone(&ran);

        // Act
        let queued = with_connection_lease(permit, async {
            spawn_owned_blocking(move || observed.store(true, std::sync::atomic::Ordering::Release))
        })
        .await;
        queued.abort();
        release_tx.send(()).unwrap();
        occupied.await.unwrap();
        assert!(queued.await.unwrap_err().is_cancelled());
        let released = tokio::time::timeout(
            Duration::from_secs(10),
            Arc::clone(&admission).acquire_owned(),
        )
        .await
        .unwrap()
        .unwrap();

        // Assert
        assert!(!ran.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(released.num_permits(), 1);
    });
}
