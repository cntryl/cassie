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

fn transaction_with_private_sequence() -> (
    crate::midge::Midge,
    CassieSession,
    crate::runtime::QueryExecutionControls,
) {
    let midge = crate::midge::Midge::new_in_memory("postgres").expect("memory Midge");
    midge.bootstrap_families().expect("bootstrap");
    let session = CassieSession::new("root".to_owned(), Some("postgres".to_owned()));
    session.begin_transaction(None).expect("begin");
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let control_scope = crate::midge::adapter::PublicationControlScope::enter(Some(&controls));
    let ddl_scope = midge
        .enter_transactional_ddl_scope("postgres", session.transaction_storage_overlay(), true)
        .expect("private sequence scope");
    let mut sequence =
        crate::catalog::SequenceMeta::new("epoch_private_seq", crate::types::DataType::BigInt);
    sequence.object_id = 481;
    midge.put_sequence(&sequence).expect("stage sequence");
    midge
        .update_sequence_retrying("epoch_private_seq", |metadata| {
            metadata.advance().map_err(Into::into)
        })
        .expect("advance private cursor");
    drop(ddl_scope);
    session.create_savepoint("before_fork").expect("savepoint");
    drop(control_scope);
    assert!(session.transaction.lock().savepoints[0]
        .storage_overlay
        .has_sequence_cursors());
    (midge, session, controls)
}

#[test]
fn should_preserve_fork_when_savepoint_cursor_pruning_is_canceled() {
    // Arrange
    let (_midge, session, normal) = transaction_with_private_sequence();
    let normal_scope = crate::midge::adapter::PublicationControlScope::enter(Some(&normal));
    let batch = stage_fork(&session, "kept");
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let canceled = crate::runtime::QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();

    // Act
    let restore = {
        let _canceled_scope =
            crate::midge::adapter::PublicationControlScope::enter(Some(&canceled));
        session.rollback_to_savepoint("before_fork")
    };
    let publication = session.publish_statement_batch(&batch);
    drop(normal_scope);

    // Assert
    assert!(matches!(restore, Err(CassieError::QueryCanceled)));
    assert!(publication.is_ok());
    assert!(session.document_change("epoch_rows", "kept").is_some());
}

#[test]
fn should_reject_old_fork_when_owner_key_restore_fails_after_savepoint_restore() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE epoch_unique (id INT UNIQUE)",
            vec![],
        )
        .expect("unique table");
    session.begin_transaction(None).expect("begin");
    let waits = std::sync::Arc::new(crate::app::TransactionUniqueWaits::default());
    session.attach_transaction_unique_waits(std::sync::Arc::clone(&waits));
    let collection = cassie.midge.canonical_collection_name("epoch_unique");
    let keys = crate::app::collect_upsert_unique_keys(
        &cassie.catalog,
        &cassie.midge,
        &collection,
        "id",
        &[json!({"id": 7})],
    );
    assert!(!keys.is_empty());
    let owner = session.transaction_unique_owner_id();
    assert!(matches!(
        waits.claim_keys(owner, &keys),
        Ok(crate::app::UniqueKeyClaim::Claimed)
    ));
    session.create_savepoint("before_fork").expect("savepoint");
    let batch = stage_fork(&session, "old");
    waits.release_owner(owner);
    assert!(matches!(
        waits.claim_keys(uuid::Uuid::new_v4(), &keys),
        Ok(crate::app::UniqueKeyClaim::Claimed)
    ));

    // Act
    let restore = session.rollback_to_savepoint("before_fork");
    let publication = session.publish_statement_batch(&batch);

    // Assert
    assert!(matches!(
        restore,
        Err(CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::DeadlockDetected,
            ..
        })
    ));
    assert!(is_stale_fork(&publication));
    assert!(session.document_change("epoch_rows", "old").is_none());
}

#[test]
fn should_preserve_fork_when_savepoint_cursor_pruning_lacks_controls() {
    // Arrange
    let (_midge, session, normal) = transaction_with_private_sequence();
    let batch = {
        let _scope = crate::midge::adapter::PublicationControlScope::enter(Some(&normal));
        stage_fork(&session, "kept")
    };

    // Act
    let restore = session.rollback_to_savepoint("before_fork");
    let publication = {
        let _scope = crate::midge::adapter::PublicationControlScope::enter(Some(&normal));
        session.publish_statement_batch(&batch)
    };

    // Assert
    assert!(matches!(restore, Err(CassieError::Unsupported(_))));
    assert!(publication.is_ok());
    assert!(session.document_change("epoch_rows", "kept").is_some());
}

#[test]
fn should_reject_old_fork_when_savepoint_pruning_unwinds() {
    // Arrange
    let (_midge, session, normal) = transaction_with_private_sequence();
    let _scope = crate::midge::adapter::PublicationControlScope::enter(Some(&normal));
    let batch = stage_fork(&session, "old");
    super::savepoints::arm_panic_after_sequence_prune_for_test();

    // Act
    let restore = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        session.rollback_to_savepoint("before_fork")
    }));
    let publication = session.publish_statement_batch(&batch);

    // Assert
    assert!(restore.is_err());
    assert!(is_stale_fork(&publication));
    assert!(session.document_change("epoch_rows", "old").is_none());
}

#[test]
fn should_publish_nested_forks_in_the_current_savepoint_epoch() {
    // Arrange
    let session = transaction_with_savepoint();
    let parent = session.fork_statement_batch().expect("parent fork");
    let nested = parent
        .session()
        .fork_statement_batch()
        .expect("nested fork");
    nested
        .session()
        .stage_document_write("epoch_rows", "nested".to_owned(), json!({"id": "nested"}))
        .expect("nested row");

    // Act
    let nested_result = parent.session().publish_statement_batch(&nested);
    let parent_result = session.publish_statement_batch(&parent);

    // Assert
    assert!(nested_result.is_ok());
    assert!(parent_result.is_ok());
    assert!(session.document_change("epoch_rows", "nested").is_some());
}

#[test]
fn should_preserve_successful_ctas_inside_explicit_transaction() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE epoch_ctas_source (id INT)",
        "INSERT INTO epoch_ctas_source VALUES (7)",
        "BEGIN",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }

    // Act
    let ctas = cassie.execute_sql(
        &session,
        "CREATE TABLE epoch_ctas_result AS SELECT id FROM epoch_ctas_source",
        vec![],
    );
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
    let rows = cassie
        .execute_sql(&session, "SELECT id FROM epoch_ctas_result", vec![])
        .expect("read result");

    // Assert
    assert!(ctas.is_ok());
    assert!(commit.is_ok());
    assert_eq!(rows.rows(), &[vec![crate::types::Value::Int64(7)]]);
}

#[test]
fn should_retire_fork_when_failed_ctas_restores_its_internal_savepoint() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE epoch_ctas_error_source (id INT)",
        "INSERT INTO epoch_ctas_error_source VALUES (1)",
        "BEGIN",
        "SAVEPOINT before_ctas",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let batch = stage_fork(&session, "old");
    let before = session.transaction.lock().rollback_epoch;

    // Act
    let ctas = cassie.execute_sql(
        &session,
        "CREATE TABLE epoch_ctas_error AS SELECT 1 / (id - id) AS value FROM epoch_ctas_error_source",
        vec![],
    );
    let after = session.transaction.lock().rollback_epoch;
    cassie
        .execute_sql(&session, "ROLLBACK TO before_ctas", vec![])
        .expect("recover failed statement");
    let publication = session.publish_statement_batch(&batch);

    // Assert
    assert!(ctas.is_err());
    assert!(after > before, "CTAS must restore its internal savepoint");
    assert!(is_stale_fork(&publication));
    assert!(session.document_change("epoch_rows", "old").is_none());
}
