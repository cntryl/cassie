use super::{execute, Arc, CaptureHook, Cassie, Value};
use crate::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use crate::config::CassieRuntimeConfig;

#[test]
fn should_read_replay_row_and_checkpoint_from_same_statement_snapshot() {
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
fn should_merge_staged_checkpoint_ddl_and_savepoint_changes() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "CREATE TABLE checkpoint_staged (title TEXT)");
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
    assert!(dropped.is_empty());
    assert_eq!(restored, created);
    assert!(discarded.is_empty());
}


#[test]
fn should_retain_repeatable_read_checkpoint_ddl_through_savepoint_rollback() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(&cassie, &session, "BEGIN ISOLATION LEVEL REPEATABLE READ");
    execute(&cassie, &session, "CREATE TABLE checkpoint_rr_staged (title TEXT)");
    let collection = cassie.midge.canonical_collection_name("checkpoint_rr_staged");
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
    assert!(dropped.is_empty());
    assert_eq!(restored, created);
}

#[test]
fn should_capture_checkpoint_and_relation_after_staged_ddl_publication() {
    // Arrange
    use std::sync::mpsc;
    use std::time::Duration;
    let cassie = Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
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
        execute(&ddl_cassie, &ddl_session, "CREATE TABLE checkpoint_race (title TEXT)");
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
