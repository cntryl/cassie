//! Detached statement publication across savepoint restoration.

use super::{CassieError, CassieSession};
use serde_json::json;
use std::sync::mpsc;
use std::time::Duration;

fn transaction_with_savepoint() -> CassieSession {
    let session = CassieSession::new("root".to_owned(), None);
    session.begin_transaction(None).expect("begin");
    session.create_savepoint("before_fork").expect("savepoint");
    session
}

fn stage_fork(session: &CassieSession, id: &str) -> super::StatementMutationBatch {
    let batch = session.fork_statement_batch().expect("statement fork");
    batch
        .session()
        .stage_document_write("epoch_rows", id.to_owned(), json!({"id": id}))
        .expect("stage detached row");
    batch
}

fn is_stale_fork(result: &Result<(), CassieError>) -> bool {
    matches!(result, Err(CassieError::Execution(message))
        if message == "session transaction changed while statement was executing")
}

#[test]
fn should_reject_detached_fork_after_identical_savepoint_restoration() {
    // Arrange
    let session = transaction_with_savepoint();
    let worker_session = session.clone();
    let (staged_tx, staged_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let batch = stage_fork(&worker_session, "old");
        staged_tx.send(()).expect("notify staged");
        resume_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("resume publication");
        worker_session.publish_statement_batch(&batch)
    });
    staged_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("fork staged before rollback");

    // Act
    session
        .rollback_to_savepoint("before_fork")
        .expect("restore identical savepoint");
    resume_tx.send(()).expect("release fork");
    let result = worker.join().expect("fork worker");

    // Assert
    assert!(is_stale_fork(&result));
    assert!(session.document_change("epoch_rows", "old").is_none());
}

#[test]
fn should_publish_fresh_fork_after_savepoint_restoration() {
    // Arrange
    let session = transaction_with_savepoint();
    let old = stage_fork(&session, "old");
    session
        .rollback_to_savepoint("before_fork")
        .expect("restore savepoint");
    let fresh = stage_fork(&session, "fresh");

    // Act
    let old_result = session.publish_statement_batch(&old);
    let fresh_result = session.publish_statement_batch(&fresh);

    // Assert
    assert!(is_stale_fork(&old_result));
    assert!(fresh_result.is_ok());
    assert!(session.document_change("epoch_rows", "old").is_none());
    assert!(session.document_change("epoch_rows", "fresh").is_some());
}

#[test]
fn should_preserve_fork_after_missing_savepoint_error() {
    // Arrange
    let session = transaction_with_savepoint();
    let batch = stage_fork(&session, "kept");

    // Act
    let restore = session.rollback_to_savepoint("missing");
    let publication = session.publish_statement_batch(&batch);

    // Assert
    assert!(matches!(
        restore,
        Err(CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::InvalidSavepoint,
            ..
        })
    ));
    assert!(publication.is_ok());
    assert!(session.document_change("epoch_rows", "kept").is_some());
}

#[test]
fn should_leave_savepoint_unchanged_when_rollback_epoch_is_exhausted() {
    // Arrange
    let session = transaction_with_savepoint();
    session
        .stage_document_write("epoch_rows", "parent".to_owned(), json!({"id": "parent"}))
        .expect("parent row after savepoint");
    session.transaction.lock().rollback_epoch = u64::MAX;
    let batch = stage_fork(&session, "fork");

    // Act
    let restore = session.rollback_to_savepoint("before_fork");
    let publication = session.publish_statement_batch(&batch);

    // Assert
    assert!(matches!(restore, Err(CassieError::ResourceLimit(_))));
    assert_eq!(session.transaction.lock().rollback_epoch, u64::MAX);
    assert!(session.document_change("epoch_rows", "parent").is_some());
    assert!(publication.is_ok());
    assert!(session.document_change("epoch_rows", "fork").is_some());
}
