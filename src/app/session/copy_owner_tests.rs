//! Transaction-owned COPY descriptor and storage fences.

use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in ["BEGIN", "CREATE TABLE copy_owner_fence (id INT)"] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    (cassie, session)
}

fn stage_storage_only(cassie: &Cassie, session: &CassieSession) {
    let _scope = cassie
        .midge
        .enter_transactional_ddl_scope(
            session
                .current_database()
                .unwrap_or(&cassie.default_database),
            session.transaction_storage_overlay().unwrap(),
            true,
        )
        .expect("DDL storage scope");
    let mut sequence = crate::catalog::SequenceMeta::new(
        "copy_owner_storage_only",
        crate::types::DataType::BigInt,
    );
    sequence.object_id = 9001;
    cassie
        .midge
        .put_sequence(&sequence)
        .expect("storage-only change");
}

#[test]
fn should_reject_a_copy_owner_from_a_retired_transaction_generation() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls)
        .expect("owner");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("new transaction");

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_err());
}

#[test]
fn should_reject_storage_only_changes_before_copy_fork_capture() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls)
        .expect("owner");
    stage_storage_only(&cassie, &session);

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_err());
}

#[test]
fn should_reject_storage_only_changes_after_copy_fork_capture() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls)
        .expect("owner");
    let fork = session
        .fork_copy_statement_batch(&owner, &controls)
        .expect("fork");
    stage_storage_only(&cassie, &session);

    // Act
    let publication = session.publish_statement_batch(&fork);

    // Assert
    assert!(publication.is_err());
}

#[test]
fn should_reject_a_changed_copy_parent_after_fork_without_rewinding_sequence() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "BEGIN",
        "CREATE TABLE copy_owner_fork (id SERIAL PRIMARY KEY)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let controls = controls();
    let _controls = crate::midge::adapter::PublicationControlScope::enter(Some(&controls));
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fork", &controls)
        .expect("owner");
    let fork = session
        .fork_copy_statement_batch(&owner, &controls)
        .expect("fork");
    let fork_scope = cassie
        .midge
        .enter_transactional_ddl_scope(
            session
                .current_database()
                .unwrap_or(&cassie.default_database),
            fork.session().transaction_storage_overlay().unwrap(),
            false,
        )
        .expect("fork storage scope");
    let first = cassie
        .midge
        .update_sequence_retrying("copy_owner_fork_id_seq", |value| {
            value.advance().map_err(Into::into)
        })
        .expect("fork sequence")
        .0;
    drop(fork_scope);
    stage_storage_only(&cassie, &session);

    // Act
    let publication = session.publish_statement_batch(&fork);
    let _parent_scope = cassie
        .midge
        .enter_transactional_ddl_scope(
            session
                .current_database()
                .unwrap_or(&cassie.default_database),
            session.transaction_storage_overlay().unwrap(),
            false,
        )
        .expect("parent storage scope");
    let next = cassie
        .midge
        .update_sequence_retrying("copy_owner_fork_id_seq", |value| {
            value.advance().map_err(Into::into)
        })
        .expect("next sequence")
        .0;

    // Assert
    assert!(publication.is_err());
    assert_eq!((first, next), (1, 2));
    assert_eq!(
        session.transaction_write_collections(),
        Vec::<String>::new()
    );
}

#[test]
fn should_use_the_default_database_for_a_session_without_an_explicit_database() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = CassieSession::new("tester".into(), None);
    session.attach_schema_context(&cassie);
    for sql in ["BEGIN", "CREATE TABLE copy_owner_default_db (id INT)"] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_default_db", &controls)
        .expect("owner");

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_ok());
}

#[test]
fn should_release_copy_owner_admission_when_the_owner_is_dropped() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let before = controls.current_query_memory_bytes();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls)
        .expect("owner");
    let retained = controls.current_query_memory_bytes();

    // Act
    drop(owner);

    // Assert
    assert!(retained > before);
    assert_eq!(controls.current_query_memory_bytes(), before);
}

#[test]
fn should_reject_recreated_physical_target_with_an_identical_zero_id_catalog() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_owner_legacy (id INT)", vec![])
        .expect("table");
    let name = "postgres.public.copy_owner_legacy";
    let schema = cassie.midge.collection_schema(name).expect("schema");
    cassie.register_collection(name, schema.clone());
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_legacy", &controls)
        .expect("physical owner");
    cassie
        .midge
        .drop_collection(name)
        .expect("drop physical target");
    cassie
        .midge
        .create_collection(name, schema.clone())
        .expect("recreate physical target");
    cassie.register_collection(name, schema);

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_err());
}

#[test]
fn should_not_pair_old_copy_snapshot_with_recreated_physical_target() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_owner_capture_race (id INT)",
            vec![],
        )
        .expect("table");
    let name = "postgres.public.copy_owner_capture_race";
    let schema = cassie.midge.collection_schema(name).expect("schema");
    cassie.register_collection(name, schema.clone());
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    let controls = controls();
    let before = {
        let overlay = session.transaction_storage_overlay().unwrap();
        let overlay = overlay.lock();
        cassie
            .midge
            .copy_physical_identity(name, &overlay, &controls)
            .expect("identity")
            .expect("stable id")
    };

    // Act: the callback synchronizes replacement after provisional resolution.
    let owner = cassie
        .capture_bound_copy_command_owner_observed(
            &session,
            "public.copy_owner_capture_race",
            &controls,
            || {
                cassie.midge.drop_collection(name)?;
                cassie.midge.create_collection(name, schema.clone())?;
                cassie.register_collection(name, schema);
                Ok(())
            },
        )
        .expect("gated recapture");

    // Assert
    let (_, _, physical_id) = owner.0.target.as_ref().expect("bound target");
    assert_ne!(
        *physical_id, before,
        "the owner must pin the replacement observed under the gate"
    );
}

#[test]
fn should_keep_a_recreated_copy_target_identity_in_the_staged_storage_overlay() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_owner_recreated (id INT, marker TEXT)",
            vec![],
        )
        .expect("published target");
    let target = crate::catalog::canonical_relation_name(
        &cassie.default_database,
        crate::catalog::DEFAULT_SCHEMA,
        "copy_owner_recreated",
    );
    let published_id = cassie
        .midge
        .row_schema(&target)
        .expect("published descriptor")
        .relation_id;
    assert!(published_id > 0);
    let controls = controls();

    // Act
    for sql in [
        "BEGIN",
        "DROP TABLE copy_owner_recreated",
        "CREATE TABLE copy_owner_recreated (payload TEXT)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("replace target");
    }
    let overlay = session.transaction_storage_overlay().unwrap();
    let descriptor = {
        let _scope = cassie
            .midge
            .enter_transactional_ddl_scope(&cassie.default_database, Arc::clone(&overlay), false)
            .expect("staged descriptor scope");
        cassie
            .midge
            .row_schema(&target)
            .expect("replacement descriptor")
    };
    let physical_id = cassie
        .midge
        .copy_physical_identity(&target, &overlay.lock(), &controls)
        .expect("staged physical identity");

    // Assert
    assert!(
        descriptor.relation_id > 0,
        "replacement descriptor: {descriptor:?}"
    );
    assert_ne!(descriptor.relation_id, published_id);
    assert_eq!(descriptor.fields.len(), 1);
    assert_eq!(descriptor.fields[0].name, "payload");
    assert_eq!(physical_id, Some(descriptor.relation_id));
    let (commit_gates, _) = cassie.transaction_commit_write_gates(&session).unwrap();
    assert!(
        commit_gates.contains(&target),
        "pure DDL retains its target gate"
    );
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_recreated", &controls)
        .expect("replacement COPY owner");
    assert_eq!(owner.target.as_ref().map(|(_, _, id)| *id), physical_id);
    let fork = session
        .fork_copy_statement_batch(&owner, &controls)
        .expect("replacement fork");
    drop(fork);
    drop(owner);
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");
    assert_eq!(
        cassie
            .midge
            .row_schema(&target)
            .expect("restored descriptor")
            .relation_id,
        published_id
    );
}

#[test]
fn should_punt_a_recreated_published_copy_target_under_repeatable_read() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE copy_rr_recreated (id INT)",
        "INSERT INTO copy_rr_recreated VALUES (7)",
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
        "SELECT count(*) FROM copy_rr_recreated",
        "DROP TABLE copy_rr_recreated",
        "CREATE TABLE copy_rr_recreated (payload TEXT)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("replacement setup");
    }
    let controls = controls();

    // Act
    let owner = cassie.capture_copy_command_owner(&session, "public.copy_rr_recreated", &controls);
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");

    // Assert
    assert!(matches!(
        owner,
        Err(CassieError::Unsupported(ref message))
            if message == "COPY into a recreated published target is not supported under REPEATABLE READ"
    ));
    let restored = cassie
        .execute_sql(&session, "SELECT id FROM copy_rr_recreated", vec![])
        .expect("original target");
    assert_eq!(restored.rows(), &[vec![crate::types::Value::Int64(7)]]);
}

#[test]
fn should_keep_new_repeatable_read_copy_targets_supported() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
        "SELECT 1",
        "CREATE TABLE copy_rr_new (payload TEXT)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("new target");
    }
    let controls = controls();

    // Act
    let owner = cassie.capture_copy_command_owner(&session, "public.copy_rr_new", &controls);

    // Assert
    assert!(owner.is_ok());
}

#[test]
fn should_reject_database_qualified_copy_target_without_retiring_the_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();

    // Act
    let rejected =
        cassie.capture_copy_command_owner(&session, "postgres.public.copy_owner_fence", &controls);
    let accepted =
        cassie.capture_copy_command_owner(&session, "public.copy_owner_fence", &controls);

    // Assert
    assert!(matches!(
        rejected,
        Err(CassieError::Unsupported(ref message))
            if message == "cross-database relation references are not supported"
    ));
    assert!(accepted.is_ok());
}

#[test]
fn should_admit_copy_search_path_snapshot_before_allocating() {
    // Arrange
    let session = CassieSession::new("tester".into(), None);
    session.set_search_path(vec!["staged".into(), "public".into()]);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let snapshot = copy_search_path(&session, &controls);
    // Assert
    assert!(snapshot.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_quote_heavy_copy_schema_canonicalization() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 10,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let canonical = copy_canonical_schema("db", "\"\"\"\"", &controls);
    // Assert
    assert!(canonical.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_copy_owner_check_while_waiting_for_session_lock() {
    // Arrange
    let (cassie, session) = fixture();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls)
        .expect("owner");
    let lock = session.transaction.lock();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::sync_channel(1);
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let worker_session = session.clone();
    let worker = std::thread::spawn(move || {
        let mut waiting = Some(waiting_tx);
        let result = owner.ensure_current_transaction_observed(
            &worker_session,
            &controls,
            || {
                if let Some(sender) = waiting.take() {
                    sender.send(()).expect("wait observer");
                }
            },
            || {},
        );
        result_tx.send(result).expect("result observer");
    });
    waiting_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("owner check reached contended lock");

    // Act
    cancellation.cancel();
    let result = result_rx.recv_timeout(std::time::Duration::from_secs(5));
    drop(lock);
    worker.join().expect("owner-check worker");

    // Assert
    assert!(matches!(
        result.expect("canceled before lock release"),
        Err(CassieError::QueryCanceled)
    ));
    assert!(session.transaction.try_lock().is_some());
}

#[test]
fn should_expire_copy_owner_check_while_waiting_for_session_lock() {
    // Arrange
    let (cassie, session) = fixture();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls())
        .expect("owner");
    let mut controls = controls();
    controls.deadline = Some(Instant::now() + std::time::Duration::from_secs(3));
    let lock = session.transaction.lock();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::sync_channel(1);
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let worker_session = session.clone();
    let worker = std::thread::spawn(move || {
        let mut waiting = Some(waiting_tx);
        let result = owner.ensure_current_transaction_observed(
            &worker_session,
            &controls,
            || {
                if let Some(sender) = waiting.take() {
                    sender.send(()).expect("wait observer");
                }
            },
            || {},
        );
        result_tx.send(result).expect("result observer");
    });
    waiting_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("owner check reached contended lock before deadline");

    // Act
    let result = result_rx.recv_timeout(std::time::Duration::from_secs(5));
    drop(lock);
    worker.join().expect("owner-check worker");

    // Assert
    assert!(matches!(
        result.expect("deadline before lock release"),
        Err(CassieError::DeadlineExceeded)
    ));
    assert!(session.transaction.try_lock().is_some());
}

#[test]
fn should_cancel_copy_owner_check_after_acquiring_session_lock() {
    // Arrange
    let (cassie, session) = fixture();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls)
        .expect("owner");

    // Act
    let result = owner.ensure_current_transaction_observed(
        &session,
        &controls,
        || panic!("uncontended check should not wait"),
        || cancellation.cancel(),
    );

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(session.transaction.try_lock().is_some());
}

#[test]
fn should_reject_copy_binding_when_savepoint_changes_between_owner_captures() {
    // Arrange
    let (cassie, session) = fixture();
    session.create_savepoint("copy_epoch").expect("savepoint");
    let worker_cassie = cassie.clone();
    let worker_session = session.clone();
    let (captured_tx, captured_rx) = std::sync::mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        worker_cassie
            .capture_bound_copy_command_owner_observed(
                &worker_session,
                "public.copy_owner_fence",
                &controls(),
                || {
                    captured_tx.send(()).expect("provisional capture");
                    resume_rx
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .expect("resume capture");
                    Ok(())
                },
            )
            .map(|_| ())
    });
    captured_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("provisional capture");

    // Act
    session
        .rollback_to_savepoint("copy_epoch")
        .expect("restore unchanged savepoint");
    resume_tx.send(()).expect("resume capture");
    let stale = worker.join().expect("capture worker");
    let fresh = cassie.capture_copy_command_owner(&session, "public.copy_owner_fence", &controls());

    // Assert
    assert!(matches!(stale, Err(CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::SerializationFailure,
        message,
    }) if message == "COPY owner no longer matches the active transaction"));
    assert!(fresh.is_ok());
}

#[test]
fn should_reject_old_copy_owner_at_guarded_fork_after_savepoint_restore() {
    // Arrange
    let (cassie, session) = fixture();
    session.create_savepoint("copy_epoch").expect("savepoint");
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls())
        .expect("owner");
    session
        .rollback_to_savepoint("copy_epoch")
        .expect("restore savepoint");

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls());

    // Assert
    assert!(matches!(fork, Err(CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::SerializationFailure,
        message,
    }) if message == "COPY owner no longer matches the active transaction"));
}

#[test]
fn should_preserve_failed_transaction_precedence_at_copy_fork() {
    // Arrange
    let (cassie, session) = fixture();
    session.create_savepoint("copy_epoch").expect("savepoint");
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls())
        .expect("owner");
    session
        .rollback_to_savepoint("copy_epoch")
        .expect("restore savepoint");
    session.mark_transaction_failed();

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls());

    // Assert
    assert!(matches!(
        fork,
        Err(CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::FailedTransaction,
            ..
        })
    ));
}

#[test]
fn should_preserve_copy_owner_after_missing_savepoint() {
    // Arrange
    let (cassie, session) = fixture();
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls())
        .expect("owner");

    // Act
    let restore = session.rollback_to_savepoint("missing");
    let valid = owner.ensure_current_transaction(&session, &controls());

    // Assert
    assert!(matches!(
        restore,
        Err(CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::InvalidSavepoint,
            ..
        })
    ));
    assert!(valid.is_ok());
}

#[test]
fn should_preserve_copy_owner_after_savepoint_release() {
    // Arrange
    let (cassie, session) = fixture();
    session.create_savepoint("copy_epoch").expect("savepoint");
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &controls())
        .expect("owner");

    // Act
    session.release_savepoint("copy_epoch").expect("release");
    let valid = session.fork_copy_statement_batch(&owner, &controls());

    // Assert
    assert!(valid.is_ok());
}

#[test]
fn should_preserve_copy_owner_when_savepoint_pruning_is_canceled() {
    // Arrange
    let (cassie, session) = fixture();
    let normal = controls();
    {
        let _controls = crate::midge::adapter::PublicationControlScope::enter(Some(&normal));
        let _ddl = cassie
            .midge
            .enter_transactional_ddl_scope(
                &cassie.default_database,
                session.transaction_storage_overlay().unwrap(),
                true,
            )
            .expect("private sequence scope");
        let mut sequence = crate::catalog::SequenceMeta::new(
            "copy_owner_private_seq",
            crate::types::DataType::BigInt,
        );
        sequence.object_id = 481;
        cassie
            .midge
            .put_sequence(&sequence)
            .expect("stage sequence");
        cassie
            .midge
            .update_sequence_retrying("copy_owner_private_seq", |metadata| {
                metadata.advance().map_err(Into::into)
            })
            .expect("advance private cursor");
    }
    session.create_savepoint("copy_epoch").expect("savepoint");
    assert!(session
        .transaction
        .lock()
        .savepoints
        .last()
        .expect("savepoint")
        .storage_overlay
        .has_sequence_cursors());
    let owner = cassie
        .capture_copy_command_owner(&session, "public.copy_owner_fence", &normal)
        .expect("owner");
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let canceled = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();

    // Act
    let restore = {
        let _controls = crate::midge::adapter::PublicationControlScope::enter(Some(&canceled));
        session.rollback_to_savepoint("copy_epoch")
    };
    let valid = owner.ensure_current_transaction(&session, &normal);

    // Assert
    assert!(matches!(restore, Err(CassieError::QueryCanceled)));
    assert!(valid.is_ok());
}

#[test]
fn should_reject_copy_target_binding_after_savepoint_restore() {
    // Arrange
    let (cassie, session) = fixture();
    session.create_savepoint("copy_epoch").expect("savepoint");
    let controls = controls();
    let mut owner = cassie
        .capture_copy_command_view(&session, &controls)
        .expect("owner");
    session
        .rollback_to_savepoint("copy_epoch")
        .expect("restore savepoint");
    let publication = Arc::clone(&owner.publication_guard);
    let _publication = publication.read();

    // Act
    let result = owner.bind_target_guarded(&session, "postgres.public.copy_owner_fence", &controls);

    // Assert
    assert!(matches!(result, Err(CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::SerializationFailure,
        message,
    }) if message == "COPY owner no longer matches the active transaction"));
}

#[test]
fn should_report_stale_copy_target_binding_for_current_failed_owner() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let mut owner = cassie
        .capture_copy_command_view(&session, &controls)
        .expect("owner");
    session.mark_transaction_failed();
    let publication = Arc::clone(&owner.publication_guard);
    let _publication = publication.read();

    // Act
    let result = owner.bind_target_guarded(&session, "postgres.public.copy_owner_fence", &controls);

    // Assert
    assert!(matches!(result, Err(CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::SerializationFailure,
        message,
    }) if message == "COPY owner no longer matches the active transaction"));
}
