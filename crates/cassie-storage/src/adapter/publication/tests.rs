//! Publication ordering and recovery quarantine before any new native write.

use std::cell::Cell;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::*;
use crate::adapter::request_test_support::TestRequest;
use cassie_core::request::RequestCancellation;
use cassie_core::types::{DataType, FieldSchema, Schema};

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

fn controls(cancellation: RequestCancellation) -> TestRequest {
    TestRequest::with_cancellation(cancellation)
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
    assert!(matches!(error, CassieError::RecoveryRequired(_)));
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
    let request = TestRequest::default();
    let captured = second.with_operation(&request.context(), || {
        submitted.set(true);
        Ok(())
    });
    resume_tx.send(()).expect("finish unknown operation");
    let original = worker.join().expect("original publisher");

    // Assert
    assert!(matches!(blocked, Err(AdapterCommitError::RecoveryRequired)));
    assert!(matches!(captured, Err(CassieError::RecoveryRequired(_))));
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
    let cancellation = RequestCancellation::new();
    let controls = controls(cancellation.clone());
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let submitted = Cell::new(false);
        let mut waiting = Some((waiting_tx, resume_rx));
        let result = worker_gate.publish_with_observer(
            Some(&controls.context()),
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
    resume_tx.send(()).expect("resume canceled waiter");
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("retire without releasing gate");
    let (result, submitted) = worker.join().expect("waiter");
    drop(held);

    // Assert
    assert!(matches!(
        result,
        Err(AdapterCommitError::Cassie(CassieError::QueryCanceled))
    ));
    assert!(!submitted);
}

#[test]
fn should_use_the_current_publication_request_instead_of_a_capture_request() {
    // Arrange
    let coordinator = PublicationCoordinator::default();
    let cancellation = RequestCancellation::new();
    let first = controls(cancellation.clone());
    let gate = {
        let _capture_scope = NativePublicationControlScope::enter(Some(&first.context()));
        coordinator.family(1)
    };
    cancellation.cancel();
    let current = TestRequest::default();
    let submitted = Cell::new(false);

    // Act
    let _commit_scope = NativePublicationControlScope::enter(Some(&current.context()));
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
    let query = TestRequest::default();

    // Act
    let blocked = {
        let _rest =
            NativePublicationControlScope::enter_for_rest(&RequestCancellation::new(), deadline);
        let _sql = NativePublicationControlScope::enter(Some(&query.context()));
        let _nested = NativePublicationControlScope::enter(None);
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
    let cancellation = RequestCancellation::new();
    let query = TestRequest::default();
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .expect("hang guard");
    let submitted = Cell::new(false);
    let _rest = NativePublicationControlScope::enter_for_rest(&cancellation, deadline);
    cancellation.cancel();

    // Act
    let blocked = gate.with_operation(&query.context(), || {
        submitted.set(true);
        Ok(())
    });

    // Assert
    assert!(matches!(blocked, Err(CassieError::QueryCanceled)));
    assert!(!submitted.get());
}

#[test]
fn should_keep_a_competing_document_publisher_out_of_the_validation_window() {
    // Arrange
    let midge = Arc::new(Midge::new_in_memory("postgres").expect("memory fixture"));
    midge.ensure_families_ready().expect("families");
    for table in ["fenced_rows", "competing_rows"] {
        midge
            .create_collection(
                table,
                Schema {
                    fields: vec![FieldSchema {
                        name: "value".to_owned(),
                        data_type: DataType::Int,
                        nullable: true,
                    }],
                },
            )
            .expect("table");
    }
    midge
        .put_document(
            "fenced_rows",
            Some("one".to_owned()),
            serde_json::json!({"value": 1}),
        )
        .expect("seed");
    let identity = midge.row_schema("fenced_rows").expect("row schema").relation_id;
    let origin = TestRequest::default();
    let fence =
crate::adapter::RetainedRowFence::capture(&midge, "postgres", &origin.context())
            .expect("old row fence");
    let request = TestRequest::default();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let other = Arc::clone(&midge);
    let other_events = Arc::clone(&events);

    // Act
    let worker = midge
        .with_database_publication("postgres", &request.context(), || {
            let worker = std::thread::spawn(move || {
                NEXT_PUBLICATION.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |boundary| {
                        assert!(boundary == PublicationBoundary::WaitingForGate);
                        waiting_tx.send(()).expect("native commit is waiting");
                    }));
                });
                let current = TestRequest::default();
                let _scope = NativePublicationControlScope::enter(Some(&current.context()));
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
            fence.validate_rows([Ok(("fenced_rows", identity, "one"))], &request.context())?;
            events.lock().expect("events").push("validated");
            midge.put_document(
                "fenced_rows",
                Some("one".to_owned()),
                serde_json::json!({"value": 3}),
            )?;
            events.lock().expect("events").push("published");
            Ok::<_, CassieError>(worker)
        })
        .expect("complete protected publication window");
    worker.join().expect("competing publisher");

    // Assert
    assert_eq!(
        *events.lock().expect("events"),
        ["validated", "published", "competing"]
    );
    assert_eq!(
        midge
            .get_document("fenced_rows", "one")
            .expect("own row")
            .expect("row")
            .payload["value"],
        3
    );
    assert_eq!(
        midge
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
    let request = TestRequest::default();
    let other = Arc::clone(&midge);
    let (waiting_tx, waiting_rx) = mpsc::channel();

    // Act
    let (mut fence, read, worker) = midge
        .with_database_publication("postgres", &request.context(), || {
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
                let request = TestRequest::default();
                let _scope = NativePublicationControlScope::enter(Some(&request.context()));
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
            Ok::<_, CassieError>((fence, read, worker))
        })
        .expect("paired capture");
    worker.join().expect("publisher");
    let visible = read.get(key).expect("retained read").expect("old row");
    fence.set_conflict_policy(cntryl_midge::ConflictPolicy::LastWriteWins);
    fence
        .assert_expected_value(
            key.to_vec(),
            &crate::adapter::transactions::ExpectedStorageValue::Present(visible.to_vec()),
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
