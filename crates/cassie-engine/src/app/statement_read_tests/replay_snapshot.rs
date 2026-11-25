use super::{execute, Arc, CaptureHook, Cassie, Value};
use crate::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use crate::config::CassieRuntimeConfig;

#[test]
fn should_reject_new_empty_document_ids_before_publishing_replay() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let reader = cassie.create_session("reader", None);
    execute(&cassie, &reader, "CREATE TABLE replay_ids (val INTEGER)");
    let projection = cassie.midge.canonical_collection_name("replay_ids");
    let make_batch = |kind: &str, id: &str, payload, position| ProjectionReplayBatch {
        projection: projection.clone(),
        source_identity: "orders".to_owned(),
        batch_id: format!("batch-{kind}"),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: format!("event-{kind}"),
            checkpoint: format!("checkpoint-{kind}"),
            position: Some(position),
            document_id: id.to_owned(),
            payload,
        }],
    };
    let accepted = make_batch("control", " ", Some(serde_json::json!({"val": 11})), 1);
    cassie
        .replay_projection_batch_strict(accepted.clone())
        .unwrap();
    let mut rejected = make_batch("empty-put", "", Some(serde_json::json!({"val": 37})), 3);
    let valid_before_rejection =
        make_batch("other", "other", Some(serde_json::json!({"val": 21})), 2);
    rejected
        .events
        .insert(0, valid_before_rejection.events.into_iter().next().unwrap());
    let rejected_delete = make_batch("empty-delete", "", None, 2);

    // Act
    let put = cassie.replay_projection_batch_strict(rejected.clone());
    let retry = cassie.replay_projection_batch_strict(rejected);
    let delete = cassie.replay_projection_batch_strict(rejected_delete);
    let accepted_retry = cassie.replay_projection_batch_strict(accepted).unwrap();
    let metadata = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let point = cassie.midge.get_document(&projection, "").unwrap();
    let scan = cassie.midge.scan_documents(&projection).unwrap();
    let rows = execute(&cassie, &reader, "SELECT val FROM replay_ids ORDER BY val");

    // Assert
    for result in [put, retry, delete] {
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("document id cannot be empty"));
    }
    assert_eq!(accepted_retry.applied_event_count, 0);
    assert_eq!(accepted_retry.skipped_duplicate_count, 1);
    assert_eq!(
        metadata.source_checkpoint.as_deref(),
        Some("checkpoint-control")
    );
    for event in ["event-other", "event-empty-put", "event-empty-delete"] {
        assert!(!cassie
            .midge
            .has_projection_event(&projection, "orders", event)
            .unwrap());
    }
    assert!(point.is_none());
    assert_eq!(scan.len(), 1);
    assert_eq!(scan[0].id, " ");
    assert_eq!(rows, vec![vec![Value::Int64(11)]]);
}

#[test]
fn should_read_replay_row_with_checkpoint_from_same_statement_snapshot() {
    // Arrange
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
    cassie.startup().unwrap();
    let reader = cassie.create_session("reader", None);
    execute(
        &cassie,
        &reader,
        "CREATE TABLE replay_snapshot_rows (title TEXT)",
    );
    let projection = cassie
        .midge
        .canonical_collection_name("replay_snapshot_rows");
    let batch = |position: u64, title: &str| ProjectionReplayBatch {
        projection: projection.clone(),
        source_identity: "orders".to_string(),
        batch_id: format!("batch-{position}"),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: format!("event-{position}"),
            checkpoint: format!("checkpoint-{position}"),
            position: Some(position),
            document_id: "doc-1".to_string(),
            payload: Some(serde_json::json!({"title": title})),
        }],
    };
    cassie.replay_projection_batch(batch(1, "before")).unwrap();
    let second = batch(2, "after");
    let writer = Arc::clone(&cassie);
    let _hook = CaptureHook::install(move || {
        std::thread::spawn(move || writer.replay_projection_batch(second))
            .join()
            .unwrap()
            .unwrap();
    });
    let sql = format!(
        "SELECT r.title, c.source_checkpoint FROM replay_snapshot_rows r CROSS JOIN pg_catalog.pg_projection_checkpoints c WHERE c.collection = '{projection}'"
    );

    // Act
    let captured = execute(&cassie, &reader, &sql);
    let fresh = execute(&cassie, &reader, &sql);

    // Assert
    assert_eq!(
        captured,
        vec![vec![
            Value::String("before".to_string()),
            Value::String("checkpoint-1".to_string())
        ]]
    );
    assert_eq!(
        fresh,
        vec![vec![
            Value::String("after".to_string()),
            Value::String("checkpoint-2".to_string())
        ]]
    );
}

#[test]
fn should_merge_staged_checkpoint_ddl_across_savepoint_changes() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "CREATE TABLE checkpoint_staged (title TEXT)",
    );
    let collection = cassie.midge.canonical_collection_name("checkpoint_staged");
    let sql = format!("SELECT collection FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{collection}'");
    let created = execute(&cassie, &session, &sql);
    execute(&cassie, &session, "SAVEPOINT before_drop");

    // Act
    execute(&cassie, &session, "DROP TABLE checkpoint_staged");
    let dropped = execute(&cassie, &session, &sql);
    execute(&cassie, &session, "ROLLBACK TO SAVEPOINT before_drop");
    let restored = execute(&cassie, &session, &sql);
    execute(&cassie, &session, "ROLLBACK");
    let discarded = execute(&cassie, &session, &sql);

    // Assert
    assert_eq!(created, vec![vec![Value::String(collection.clone())]]);
    assert_eq!(dropped, Vec::<Vec<Value>>::new());
    assert_eq!(restored, created);
    assert_eq!(discarded, Vec::<Vec<Value>>::new());
}

#[test]
fn should_retain_repeatable_read_checkpoint_ddl_through_savepoint_rollback() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(&cassie, &session, "BEGIN ISOLATION LEVEL REPEATABLE READ");
    execute(
        &cassie,
        &session,
        "CREATE TABLE checkpoint_rr_staged (title TEXT)",
    );
    let collection = cassie
        .midge
        .canonical_collection_name("checkpoint_rr_staged");
    let sql = format!("SELECT collection FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{collection}'");
    let created = execute(&cassie, &session, &sql);
    execute(&cassie, &session, "SAVEPOINT before_drop");

    // Act
    execute(&cassie, &session, "DROP TABLE checkpoint_rr_staged");
    let dropped = execute(&cassie, &session, &sql);
    execute(&cassie, &session, "ROLLBACK TO SAVEPOINT before_drop");
    let restored = execute(&cassie, &session, &sql);
    execute(&cassie, &session, "ROLLBACK");

    // Assert
    assert_eq!(created, vec![vec![Value::String(collection)]]);
    assert_eq!(dropped, Vec::<Vec<Value>>::new());
    assert_eq!(restored, created);
}

#[test]
fn should_capture_consistent_checkpoint_view_after_staged_ddl_publication() {
    // Arrange
    use std::sync::mpsc;
    use std::time::Duration;
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(&cassie, &session, "BEGIN");
    let (staged_tx, staged_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let ddl_cassie = Arc::clone(&cassie);
    let ddl_session = session.clone();
    let ddl = std::thread::spawn(move || {
        crate::executor::install_checkpoint_ddl_stage_hook(move || {
            staged_tx.send(()).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        });
        execute(
            &ddl_cassie,
            &ddl_session,
            "CREATE TABLE checkpoint_race (title TEXT)",
        );
    });
    staged_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (waiting_tx, waiting_rx) = mpsc::channel();
    let reader_cassie = Arc::clone(&cassie);
    let reader_session = session.clone();
    let collection = cassie.midge.canonical_collection_name("checkpoint_race");
    let reader = std::thread::spawn(move || {
        super::install_checkpoint_capture_wait_hook(move || waiting_tx.send(()).unwrap());
        let sql = format!("SELECT COUNT(*) FROM checkpoint_race r CROSS JOIN pg_catalog.pg_projection_checkpoints c WHERE c.collection = '{collection}'");
        execute(&reader_cassie, &reader_session, &sql)
    });

    // Act
    waiting_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    resume_tx.send(()).unwrap();
    ddl.join().unwrap();
    let rows = reader.join().unwrap();

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(0)]]);
    execute(&cassie, &session, "ROLLBACK");
}

#[test]
fn should_complete_nested_ctas_over_checkpoint_view_under_publication_fence() {
    // Arrange
    use std::sync::mpsc;
    use std::time::Duration;
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(&cassie, &session, "BEGIN");
    execute(
        &cassie,
        &session,
        "CREATE TABLE checkpoint_nested (title TEXT)",
    );
    let (done_tx, done_rx) = mpsc::channel();
    let worker_cassie = Arc::clone(&cassie);
    let worker_session = session.clone();

    // Act
    std::thread::spawn(move || {
        let rows = execute(
            &worker_cassie,
            &worker_session,
            "CREATE TABLE checkpoint_copy AS SELECT collection FROM pg_catalog.pg_projection_checkpoints",
        );
        done_tx.send(rows).unwrap();
    });
    let completed = done_rx.recv_timeout(Duration::from_secs(30));

    // Assert
    assert!(completed.is_ok());
    execute(&cassie, &session, "ROLLBACK");
}

fn checkpoint_collections(cassie: &Cassie, session: &super::CassieSession) -> Vec<Vec<Value>> {
    execute(
        cassie,
        session,
        "SELECT collection FROM pg_catalog.pg_projection_checkpoints ORDER BY collection",
    )
}

#[test]
fn should_list_remaining_checkpoints_after_autocommit_drop() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE checkpoint_kept (title TEXT)",
    );
    execute(
        &cassie,
        &session,
        "CREATE TABLE checkpoint_dropped (title TEXT)",
    );
    let kept = cassie.midge.canonical_collection_name("checkpoint_kept");

    // Act
    execute(&cassie, &session, "DROP TABLE checkpoint_dropped");
    let rows = checkpoint_collections(&cassie, &session);

    // Assert
    assert_eq!(rows, vec![vec![Value::String(kept)]]);
}

#[test]
fn should_list_renamed_table_checkpoint_under_its_new_name() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE checkpoint_old_name (title TEXT)",
    );
    let renamed = cassie
        .midge
        .canonical_collection_name("checkpoint_new_name");

    // Act
    execute(
        &cassie,
        &session,
        "ALTER TABLE checkpoint_old_name RENAME TO checkpoint_new_name",
    );
    let rows = checkpoint_collections(&cassie, &session);

    // Assert
    assert_eq!(rows, vec![vec![Value::String(renamed)]]);
}

#[test]
fn should_read_own_created_table_in_repeatable_read_transaction() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(&cassie, &session, "BEGIN ISOLATION LEVEL REPEATABLE READ");
    execute(
        &cassie,
        &session,
        "CREATE TABLE checkpoint_rr_own (title TEXT)",
    );
    execute(
        &cassie,
        &session,
        "INSERT INTO checkpoint_rr_own (title) VALUES ('own')",
    );

    // Act
    let rows = execute(&cassie, &session, "SELECT title FROM checkpoint_rr_own");
    execute(&cassie, &session, "COMMIT");
    let committed = execute(&cassie, &session, "SELECT title FROM checkpoint_rr_own");

    // Assert
    assert_eq!(rows, vec![vec![Value::String("own".to_owned())]]);
    assert_eq!(committed, rows);
}
