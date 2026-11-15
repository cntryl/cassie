//! Authored, unrun probes for failure authority across savepoint recovery.

use super::{Cassie, CassieError, CassieSession, TransactionErrorKind};
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;
use std::sync::mpsc;
use std::time::Duration;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in ["BEGIN", "SAVEPOINT s"] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    (cassie, session)
}

#[test]
fn should_keep_recovered_savepoint_healthy_after_old_conversion_fails() {
    const CASE: &str = "old_conversion_after_savepoint";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_savepoint_tests::should_keep_recovered_savepoint_healthy_after_old_conversion_fails",
        CASE,
    ) {
        return;
    }

    // Arrange
    let (cassie, session) = fixture();
    let owner = session.transaction_unique_owner_id();
    let before = session.rollback_epoch_for_test();
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "SELECT 1",
            vec![],
            &QueryCancellationHandle::new(),
            move |_, _| {
                entered_tx.send(()).expect("old conversion");
                resume_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("resume");
                Err::<(), _>(CassieError::Execution("old conversion".into()))
            },
        )
    });
    entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("conversion entered");
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT s", vec![])
        .expect("recover");
    let recovered = session.rollback_epoch_for_test();

    // Act
    resume_tx.send(()).expect("resume old conversion");
    let old = worker.join().expect("old worker");
    let current_owner = session.transaction_unique_owner_id();
    let healthy = !session.is_transaction_failed();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(matches!(old, Err(CassieError::Execution(ref message)) if message == "old conversion"));
    assert_eq!(current_owner, owner);
    assert_eq!(recovered, before + 1);
    assert!(healthy);
    assert!(usable.is_ok());
    drop(usable);
    drop(old);
    drop(session);
    drop(cassie);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_fail_current_owner_after_own_savepoint_conversion_error() {
    // Arrange
    let (cassie, session) = fixture();
    let owner = session.transaction_unique_owner_id();
    let before = session.rollback_epoch_for_test();

    // Act
    let result: Result<(), CassieError> = cassie.execute_sql_with_cancellation_converted(
        &session,
        "ROLLBACK TO SAVEPOINT s",
        vec![],
        &QueryCancellationHandle::new(),
        |_, _| Err(CassieError::Execution("rollback conversion".into())),
    );
    let current_owner = session.transaction_unique_owner_id();
    let after = session.rollback_epoch_for_test();
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "rollback conversion")
    );
    assert_eq!(current_owner, owner);
    assert_eq!(after, before + 1);
    assert!(failed);
}

#[test]
fn should_keep_epoch_when_savepoint_does_not_exist() {
    // Arrange
    let (cassie, session) = fixture();
    let owner = session.transaction_unique_owner_id();
    let before = session.rollback_epoch_for_test();

    // Act
    let result = cassie.execute_sql(&session, "ROLLBACK TO SAVEPOINT missing", vec![]);
    let after = session.rollback_epoch_for_test();
    let current_owner = session.transaction_unique_owner_id();
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert
    assert!(matches!(
        result,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::InvalidSavepoint,
            ..
        })
    ));
    assert_eq!(current_owner, owner);
    assert_eq!(after, before);
    assert!(failed);
}

#[test]
fn should_fail_current_owner_after_savepoint_restoration_error() {
    // Arrange: make the real unique-owner restoration fail after root restore.
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE epoch_owner_unique (id INT UNIQUE)",
            vec![],
        )
        .expect("unique table");
    session.begin_transaction(None).expect("begin");
    let waits = std::sync::Arc::new(super::TransactionUniqueWaits::default());
    session.attach_transaction_unique_waits(std::sync::Arc::clone(&waits));
    let collection = cassie.midge.canonical_collection_name("epoch_owner_unique");
    let keys = super::collect_upsert_unique_keys(
        &cassie.catalog,
        &cassie.midge,
        &collection,
        "id",
        &[serde_json::json!({"id": 7})],
    );
    assert!(!keys.is_empty());
    let owner = session.transaction_unique_owner_id();
    assert!(matches!(
        waits.claim_keys(owner, &keys),
        Ok(super::UniqueKeyClaim::Claimed)
    ));
    session.create_savepoint("s").expect("savepoint");
    let before = session.rollback_epoch_for_test();
    waits.release_owner(owner);
    let other_owner = uuid::Uuid::new_v4();
    assert!(matches!(
        waits.claim_keys(other_owner, &keys),
        Ok(super::UniqueKeyClaim::Claimed)
    ));

    // Act
    let result = cassie.execute_sql(&session, "ROLLBACK TO SAVEPOINT s", vec![]);
    let current_owner = session.transaction_unique_owner_id();
    let after = session.rollback_epoch_for_test();
    let failed = session.is_transaction_failed();
    waits.release_owner(other_owner);
    session.rollback_transaction();

    // Assert
    assert!(matches!(
        result,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::DeadlockDetected,
            ..
        })
    ));
    assert_eq!(current_owner, owner);
    assert_eq!(after, before + 1);
    assert!(failed);
}

#[test]
fn should_restore_authority_when_savepoint_cursor_pruning_is_canceled() {
    // Arrange: stage a real private sequence cursor into the savepoint.
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    session.begin_transaction(None).expect("begin");
    let setup_controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    {
        let _controls =
            crate::midge::adapter::PublicationControlScope::enter(Some(&setup_controls));
        let _ddl = cassie
            .midge
            .enter_transactional_ddl_scope(
                &cassie.default_database,
                session.transaction_storage_overlay(),
                true,
            )
            .expect("private sequence scope");
        let mut sequence = crate::catalog::SequenceMeta::new(
            "failure_private_seq",
            crate::types::DataType::BigInt,
        );
        sequence.object_id = 482;
        cassie
            .midge
            .put_sequence(&sequence)
            .expect("stage sequence");
        cassie
            .midge
            .update_sequence_retrying("failure_private_seq", |metadata| {
                metadata.advance().map_err(Into::into)
            })
            .expect("advance private cursor");
    }
    session.create_savepoint("s").expect("savepoint");
    let owner = session.transaction_unique_owner_id();
    let before = session.rollback_epoch_for_test();
    let cancellation = QueryCancellationHandle::new();
    let (observed_tx, observed_rx) = mpsc::sync_channel(1);
    // The future scoped seam fires only inside the actual cursor-prune branch,
    // after epoch advancement, and cancels the operation without taking a lock.
    let _pause = session.cancel_savepoint_prune_for_test(cancellation.clone(), observed_tx);

    // Act: setup's ambient controls have been dropped before the request.
    let result = cassie.execute_sql_with_cancellation_converted(
        &session,
        "ROLLBACK TO SAVEPOINT s",
        vec![],
        &cancellation,
        |_, _| Ok(()),
    );
    let observed = observed_rx.try_recv().expect("actual cursor prune branch");
    let current_owner = session.transaction_unique_owner_id();
    let after = session.rollback_epoch_for_test();
    let failed = session.is_transaction_failed();
    session.rollback_transaction();

    // Assert: the ordinary pre-mutation error restores epoch and authority;
    // its same-owner cancellation still fails the block.
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(observed, before + 1);
    assert_eq!(current_owner, owner);
    assert_eq!(after, before);
    assert!(failed);
}

#[test]
fn should_preserve_later_recovery_after_old_savepoint_restoration_error() {
    const CASE: &str = "old_partial_savepoint_error";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_savepoint_tests::should_preserve_later_recovery_after_old_savepoint_restoration_error",
        CASE,
    ) {
        return;
    }

    // Arrange: a real unique-owner conflict makes the first restoration fail.
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE epoch_partial_unique (id INT UNIQUE)",
            vec![],
        )
        .expect("unique table");
    session.begin_transaction(None).expect("begin");
    let waits = std::sync::Arc::new(super::TransactionUniqueWaits::default());
    session.attach_transaction_unique_waits(std::sync::Arc::clone(&waits));
    let collection = cassie
        .midge
        .canonical_collection_name("epoch_partial_unique");
    let keys = super::collect_upsert_unique_keys(
        &cassie.catalog,
        &cassie.midge,
        &collection,
        "id",
        &[serde_json::json!({"id": 7})],
    );
    assert!(!keys.is_empty());
    let owner = session.transaction_unique_owner_id();
    assert!(matches!(
        waits.claim_keys(owner, &keys),
        Ok(super::UniqueKeyClaim::Claimed)
    ));
    session.create_savepoint("s").expect("savepoint");
    let before = session.rollback_epoch_for_test();
    waits.release_owner(owner);
    let other_owner = uuid::Uuid::new_v4();
    assert!(matches!(
        waits.claim_keys(other_owner, &keys),
        Ok(super::UniqueKeyClaim::Claimed)
    ));

    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let worker = std::thread::spawn(move || {
        // This future keyed hook must observe the actual restore_owner_keys
        // error after releasing the root lock, before returning it to SQL.
        let _pause = old_session.observe_savepoint_restore_error_for_test(entered_tx, resume_rx);
        old_cassie.execute_sql(&old_session, "ROLLBACK TO SAVEPOINT s", vec![])
    });
    let observed = entered_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("real restore error");
    assert_eq!(
        observed,
        (owner, before + 1, TransactionErrorKind::DeadlockDetected)
    );
    waits.release_owner(other_owner);
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT s", vec![])
        .expect("later recovery");
    let recovered = session.rollback_epoch_for_test();

    // Act
    resume_tx.send(()).expect("release old error");
    let old = worker.join().expect("old worker");
    let current_owner = session.transaction_unique_owner_id();
    let healthy = !session.is_transaction_failed();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert
    assert!(matches!(
        old,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::DeadlockDetected,
            ..
        })
    ));
    assert_eq!(current_owner, owner);
    assert_eq!(recovered, before + 2);
    assert!(healthy);
    assert!(usable.is_ok());
    drop(usable);
    drop(old);
    drop(session);
    drop(cassie);
    super::query_failure_watchdog::complete_child(CASE);
}

#[test]
fn should_not_readopt_epoch_when_stale_operation_rolls_back_to_savepoint() {
    const CASE: &str = "stale_operation_savepoint";
    if !super::query_failure_watchdog::enter_child(
        "app::query_failure_savepoint_tests::should_not_readopt_epoch_when_stale_operation_rolls_back_to_savepoint",
        CASE,
    ) {
        return;
    }

    // Arrange: the observer must report the operation's real captured pair.
    let (cassie, session) = fixture();
    let owner = session.transaction_unique_owner_id();
    let before = session.rollback_epoch_for_test();
    let old_cassie = cassie.clone();
    let old_session = session.clone();
    let (captured_tx, captured_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let _pause = super::query_lifecycle::authority_test_hooks::install_after_root_capture(
            &old_cassie,
            &old_session,
            captured_tx,
            resume_rx,
        );
        old_cassie.execute_sql_with_cancellation_converted(
            &old_session,
            "ROLLBACK TO SAVEPOINT s",
            vec![],
            &QueryCancellationHandle::new(),
            |_, _| Err::<(), _>(CassieError::Execution("stale rollback conversion".into())),
        )
    });
    let captured = captured_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("captured");
    assert_eq!(captured, Some((owner, before)));
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT s", vec![])
        .expect("independent rollback");
    let middle = session.rollback_epoch_for_test();

    // Act
    resume_tx.send(()).expect("resume stale operation");
    let result = worker.join().expect("old worker");
    let after = session.rollback_epoch_for_test();
    let current_owner = session.transaction_unique_owner_id();
    let healthy = !session.is_transaction_failed();
    let usable = cassie.execute_sql(&session, "SELECT 1", vec![]);
    session.rollback_transaction();

    // Assert: the stale operation's own rollback advances the root epoch but
    // must not adopt it and then fail the recovered transaction.
    assert!(
        matches!(result, Err(CassieError::Execution(ref message)) if message == "stale rollback conversion")
    );
    assert_eq!(current_owner, owner);
    assert_eq!(middle, before + 1);
    assert_eq!(after, middle + 1);
    assert!(healthy);
    assert!(usable.is_ok());
    drop(usable);
    drop(result);
    drop(session);
    drop(cassie);
    super::query_failure_watchdog::complete_child(CASE);
}
