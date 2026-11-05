//! Publication ordering and recovery quarantine before any new native write.

use std::cell::Cell;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;

type PublicationCallback = Box<dyn FnOnce(PublicationBoundary)>;

thread_local! {
    static NEXT_PUBLICATION: std::cell::RefCell<Option<PublicationCallback>> = const {
        std::cell::RefCell::new(None)
    };
}

pub(super) fn observe_publication(boundary: PublicationBoundary) {
    let callback = NEXT_PUBLICATION.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback(boundary);
    }
}

type ConfirmationCallback = Box<dyn FnOnce()>;

thread_local! {
    static NEXT_CONFIRMATION: std::cell::RefCell<Option<ConfirmationCallback>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn observe_confirmation() {
    let callback = NEXT_CONFIRMATION.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

pub(super) fn next_confirmation(callback: impl FnOnce() + 'static) -> impl Drop {
    struct Restore(Option<ConfirmationCallback>);
    impl Drop for Restore {
        fn drop(&mut self) {
            drop(NEXT_CONFIRMATION.with(|slot| slot.replace(self.0.take())));
        }
    }
    Restore(NEXT_CONFIRMATION.with(|slot| slot.replace(Some(Box::new(callback)))))
}

fn controls(cancellation: QueryCancellationHandle) -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    )
}

#[test]
fn should_block_new_native_publication_after_an_unknown_outcome() {
    // Arrange
    let coordinator = PublicationCoordinator::default();
    let first = coordinator.family(1);
    let second = coordinator.family(2);
    let submitted = Cell::new(0);
    let unknown = first.publish_with(None, || {
        Err(cntryl_midge::MidgeError::OutcomeUnknown(
            "unconfirmed".to_owned(),
        ))
    });

    // Act
    let blocked = second.publish_with(None, || {
        submitted.set(submitted.get() + 1);
        Ok(())
    });

    // Assert
    assert!(matches!(
        unknown,
        Err(AdapterCommitError::Native(
            cntryl_midge::MidgeError::OutcomeUnknown(_)
        ))
    ));
    let error = CassieError::from(blocked.expect_err("recovery required"));
    assert_eq!(error.descriptor().sql_state, "58030");
    assert!(error.to_string().contains("not submitted"));
    assert!(error.storage_retry_kind().is_none());
    assert_eq!(submitted.get(), 0);
}

#[test]
fn should_latch_recovery_required_before_releasing_the_original_gate() {
    // Arrange
    let coordinator = PublicationCoordinator::default();
    let first = coordinator.family(1);
    let second = coordinator.family(2);
    let (latched_tx, latched_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        first.publish_with_observer(
            None,
            || {
                Err(cntryl_midge::MidgeError::OutcomeUnknown(
                    "may apply later".to_owned(),
                ))
            },
            |boundary| {
                if boundary == PublicationBoundary::RecoveryLatched {
                    latched_tx.send(()).expect("latched under original gate");
                    resume_rx
                        .recv_timeout(Duration::from_secs(5))
                        .expect("release original gate");
                }
            },
        )
    });
    latched_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("recovery boundary");
    let submitted = Cell::new(false);

    // Act
    let blocked = second.publish_with(None, || {
        submitted.set(true);
        Ok(())
    });
    let captured = second.with_operation(&controls(QueryCancellationHandle::new()), || {
        submitted.set(true);
        Ok(())
    });
    resume_tx.send(()).expect("finish unknown operation");
    let original = worker.join().expect("original publisher");

    // Assert
    assert!(matches!(blocked, Err(AdapterCommitError::RecoveryRequired)));
    assert!(matches!(captured, Err(CassieError::Storage(_))));
    assert!(matches!(original, Err(AdapterCommitError::Native(_))));
    assert!(!submitted.get());
}

#[test]
fn should_preserve_recovery_required_after_the_last_gate_owner_retires() {
    // Arrange
    let coordinator = PublicationCoordinator::default();
    let gate = coordinator.family(1);
    let unknown = gate.publish_with(None, || {
        Err(cntryl_midge::MidgeError::OutcomeUnknown(
            "unconfirmed".to_owned(),
        ))
    });
    assert!(unknown.is_err());
    drop(gate);

    // Act
    let replacement = coordinator.family(1);
    let blocked = replacement.publish_with(None, || Ok(()));
    let new_instance = PublicationCoordinator::default()
        .family(1)
        .publish_with(None, || Ok(()));

    // Assert
    assert!(matches!(blocked, Err(AdapterCommitError::RecoveryRequired)));
    assert!(
        new_instance.is_ok(),
        "only a new coordinator initializes a clear recovery flag"
    );
}

#[test]
fn should_keep_known_not_applied_conflicts_out_of_recovery_quarantine() {
    // Arrange
    let coordinator = PublicationCoordinator::default();
    let gate = coordinator.family(1);
    let conflict = gate.publish_with(None, || {
        Err(cntryl_midge::MidgeError::WriteConflict(
            "not applied".to_owned(),
        ))
    });

    // Act
    let next = gate.publish_with(None, || Ok(()));

    // Assert
    assert!(matches!(
        conflict,
        Err(AdapterCommitError::Native(
            cntryl_midge::MidgeError::WriteConflict(_)
        ))
    ));
    assert!(next.is_ok());
}

#[test]
fn should_cancel_a_publication_wait_before_submitting_a_native_commit() {
    // Arrange
    let coordinator = PublicationCoordinator::default();
    let gate = coordinator.family(1);
    let held = gate.mutex.lock();
    let worker_gate = Arc::clone(&gate);
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let submitted = Cell::new(false);
        let mut waiting = Some((waiting_tx, resume_rx));
        let result = worker_gate.publish_with_observer(
            Some(&controls),
            || {
                submitted.set(true);
                Ok(())
            },
            |boundary| {
                if boundary == PublicationBoundary::WaitingForGate {
                    if let Some((waiting_tx, resume_rx)) = waiting.take() {
                        waiting_tx.send(()).expect("held gate boundary");
                        resume_rx
                            .recv_timeout(Duration::from_secs(5))
                            .expect("resume waiter");
                    }
                }
            },
        );
        done_tx.send(()).expect("interrupted wait retired");
        (result, submitted.get())
    });
    waiting_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("waiter failed to acquire held gate");

    // Act
    cancellation.cancel();
    resume_tx.send(()).expect("resume cancelled waiter");
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("retire without releasing gate");
    let (result, submitted) = worker.join().expect("waiter");
    drop(held);

    // Assert
    assert!(matches!(
        result,
        Err(AdapterCommitError::Cassie(CassieError::QueryCancelled))
    ));
    assert!(!submitted);
}

#[test]
fn should_use_the_current_publication_request_instead_of_a_capture_request() {
    // Arrange
    let coordinator = PublicationCoordinator::default();
    let cancellation = QueryCancellationHandle::new();
    let first = controls(cancellation.clone());
    let gate = {
        let _capture_scope = PublicationControlScope::enter(Some(&first));
        coordinator.family(1)
    };
    cancellation.cancel();
    let current = controls(QueryCancellationHandle::new());
    let submitted = Cell::new(false);

    // Act
    let _commit_scope = PublicationControlScope::enter(Some(&current));
    let result = gate.publish_current(|| {
        submitted.set(true);
        Ok(())
    });

    // Assert
    assert!(result.is_ok());
    assert!(submitted.get());
}

#[test]
fn should_preserve_the_outer_rest_deadline_inside_sql_scopes() {
    // Arrange
    let gate = PublicationCoordinator::default().family(1);
    let submitted = Cell::new(false);
    let deadline = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("past deadline");
    let query = controls(QueryCancellationHandle::new());

    // Act
    let blocked = {
        let _rest =
            PublicationControlScope::enter_for_rest(&QueryCancellationHandle::new(), deadline);
        let _sql = PublicationControlScope::enter(Some(&query));
        let _nested = PublicationControlScope::for_request(&CassieRuntimeLimits::default(), None);
        gate.publish_current(|| {
            submitted.set(true);
            Ok(())
        })
    };
    let restored = gate.publish_current(|| Ok(()));

    // Assert
    assert!(matches!(
        blocked,
        Err(AdapterCommitError::Cassie(CassieError::DeadlineExceeded))
    ));
    assert!(!submitted.get());
    assert!(restored.is_ok(), "stack scopes retire the REST deadline");
}

#[test]
fn should_preserve_rest_cancellation_for_an_explicit_snapshot_window() {
    // Arrange
    let gate = PublicationCoordinator::default().family(1);
    let cancellation = QueryCancellationHandle::new();
    let query = controls(QueryCancellationHandle::new());
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("hang guard");
    let submitted = Cell::new(false);
    let _rest = PublicationControlScope::enter_for_rest(&cancellation, deadline);
    cancellation.cancel();

    // Act
    let blocked = gate.with_operation(&query, || {
        submitted.set(true);
        Ok(())
    });

    // Assert
    assert!(matches!(blocked, Err(CassieError::QueryCancelled)));
    assert!(!submitted.get());
}

#[test]
fn should_keep_a_competing_document_publisher_out_of_the_validation_window() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE fenced_rows (value INT)",
        "CREATE TABLE competing_rows (value INT)",
    ] {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .expect("table");
    }
    cassie
        .midge
        .put_document(
            "fenced_rows",
            Some("one".to_owned()),
            serde_json::json!({"value": 1}),
        )
        .expect("seed");
    let identity = cassie
        .catalog
        .collection_storage_id("fenced_rows")
        .expect("relation identity");
    let origin = controls(QueryCancellationHandle::new());
    let fence =
        crate::midge::adapter::RetainedRowFence::capture(&cassie.midge, "postgres", &origin)
            .expect("old row fence");
    let request = controls(QueryCancellationHandle::new());
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let other = Arc::clone(&cassie.midge);
    let other_events = Arc::clone(&events);

    // Act
    let worker = cassie
        .midge
        .with_database_publication("postgres", &request, || {
            let worker = std::thread::spawn(move || {
                NEXT_PUBLICATION.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |boundary| {
                        assert!(boundary == PublicationBoundary::WaitingForGate);
                        waiting_tx.send(()).expect("native commit is waiting");
                    }));
                });
                let current = controls(QueryCancellationHandle::new());
                let _scope = PublicationControlScope::enter(Some(&current));
                other
                    .put_document(
                        "competing_rows",
                        Some("two".to_owned()),
                        serde_json::json!({"value": 2}),
                    )
                    .expect("competing document publication");
                other_events.lock().expect("events").push("competing");
            });
            waiting_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("actual competing commit reached held database gate");
            fence.validate_rows([Ok(("fenced_rows", identity, "one"))], &request)?;
            events.lock().expect("events").push("validated");
            cassie.midge.put_document(
                "fenced_rows",
                Some("one".to_owned()),
                serde_json::json!({"value": 3}),
            )?;
            events.lock().expect("events").push("published");
            Ok(worker)
        })
        .expect("complete protected publication window");
    worker.join().expect("competing publisher");

    // Assert
    assert_eq!(
        *events.lock().expect("events"),
        ["validated", "published", "competing"]
    );
    assert_eq!(
        cassie
            .midge
            .get_document("fenced_rows", "one")
            .expect("own row")
            .expect("row")
            .payload["value"],
        3
    );
    assert_eq!(
        cassie
            .midge
            .get_document("competing_rows", "two")
            .expect("other row")
            .expect("row")
            .payload["value"],
        2
    );
    assert_eq!(origin.current_query_memory_bytes(), 0);
    assert_eq!(request.current_query_memory_bytes(), 0);
}

#[test]
fn should_capture_matching_validation_views_while_a_publisher_waits() {
    // Arrange
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory adapter"));
    midge.ensure_families_ready().expect("families");
    let key = b"paired-snapshot-row";
    let mut seed = midge
        .begin_raw_database_adapter_tx("postgres", cntryl_midge::TransactionMode::ReadWrite)
        .expect("seed transaction");
    seed.put(key.to_vec(), b"before".to_vec(), None)
        .expect("seed");
    seed.commit(midge.write_options_sync())
        .expect("seed commit");
    let request = controls(QueryCancellationHandle::new());
    let other = Arc::clone(&midge);
    let (waiting_tx, waiting_rx) = mpsc::channel();

    // Act
    let (mut fence, read, worker) = midge
        .with_database_publication("postgres", &request, || {
            let fence = midge.begin_raw_database_adapter_tx(
                "postgres",
                cntryl_midge::TransactionMode::ReadWrite,
            )?;
            let worker = std::thread::spawn(move || {
                NEXT_PUBLICATION.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |boundary| {
                        assert!(boundary == PublicationBoundary::WaitingForGate);
                        waiting_tx.send(()).expect("writer reached capture gate");
                    }));
                });
                let request = controls(QueryCancellationHandle::new());
                let _scope = PublicationControlScope::enter(Some(&request));
                let mut writer = other
                    .begin_raw_database_adapter_tx(
                        "postgres",
                        cntryl_midge::TransactionMode::ReadWrite,
                    )
                    .expect("competing writer");
                writer
                    .put(key.to_vec(), b"after".to_vec(), None)
                    .expect("competing row");
                writer
                    .commit(other.write_options_sync())
                    .expect("competing commit");
            });
            waiting_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("commit boundary");
            let read = midge.begin_database_readonly_tx("postgres")?;
            Ok((fence, read, worker))
        })
        .expect("paired capture");
    worker.join().expect("publisher");
    let visible = read.get(key).expect("retained read").expect("old row");
    fence.set_conflict_policy(cntryl_midge::ConflictPolicy::LastWriteWins);
    fence
        .assert_expected_value(
            key.to_vec(),
            &crate::midge::adapter::transactions::ExpectedStorageValue::Present(visible.to_vec()),
        )
        .expect("assert retained view");
    let validation = fence.commit(midge.write_options_sync());

    // Assert
    assert_eq!(visible.as_ref(), b"before");
    assert!(matches!(
        validation,
        Err(AdapterCommitError::Native(
            cntryl_midge::MidgeError::WriteConflict(_)
        ))
    ));
}

pub(super) fn next_wait(callback: impl FnOnce() + 'static) -> impl Drop {
    struct Restore(Option<PublicationCallback>);
    impl Drop for Restore {
        fn drop(&mut self) {
            drop(NEXT_PUBLICATION.with(|slot| slot.replace(self.0.take())));
        }
    }
    Restore(NEXT_PUBLICATION.with(|slot| {
        slot.replace(Some(Box::new(move |boundary| {
            assert!(boundary == PublicationBoundary::WaitingForGate);
            callback();
        })))
    }))
}
