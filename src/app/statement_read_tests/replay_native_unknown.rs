use super::{execute, Cassie, Value};
use crate::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use crate::config::CassieRuntimeConfig;

#[test]
fn should_reconcile_committed_native_unknown_after_restart() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-replay-native-unknown-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE replay_native_unknown (title TEXT)",
    );
    let projection = cassie
        .midge
        .canonical_collection_name("replay_native_unknown");
    let batch = ProjectionReplayBatch {
        projection: projection.clone(),
        source_identity: "orders".to_string(),
        batch_id: "batch-1".to_string(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: "event-1".to_string(),
            checkpoint: "checkpoint-1".to_string(),
            position: Some(1),
            document_id: "doc-1".to_string(),
            payload: Some(serde_json::json!({"title": "committed"})),
        }],
    };
    crate::midge::adapter::inject_native_commit_unknown_after_success();

    // Act
    let result = cassie.replay_projection_batch(batch.clone());
    let fenced = cassie.midge.projection_metadata(&projection);
    let fenced_checkpoint = cassie.execute_sql(
        &session,
        "SELECT source_checkpoint FROM pg_catalog.pg_projection_checkpoints",
        vec![],
    );
    cassie.shutdown();
    drop(cassie);
    let recovered =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    recovered.startup().unwrap();
    let recovered_session = recovered.create_session("reader", None);
    let rows = execute(
        &recovered,
        &recovered_session,
        "SELECT title FROM replay_native_unknown",
    );
    let metadata = recovered
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let duplicate = recovered.replay_projection_batch(batch).unwrap();

    // Assert
    assert!(result.is_err());
    assert!(fenced.is_err());
    assert!(fenced_checkpoint.is_err());
    assert_eq!(rows, vec![vec![Value::String("committed".to_string())]]);
    assert_eq!(metadata.source_checkpoint.as_deref(), Some("checkpoint-1"));
    assert_eq!(duplicate.applied_event_count, 0);
    assert_eq!(duplicate.skipped_duplicate_count, 1);
    recovered.shutdown();
    drop(recovered);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn should_reconcile_committed_replay_when_postcommit_metadata_read_fails() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE replay_postcommit_read (title TEXT)",
    );
    execute(
        &cassie,
        &session,
        "CREATE INDEX replay_postcommit_title_idx ON replay_postcommit_read USING btree (title)",
    );
    let projection = cassie
        .midge
        .canonical_collection_name("replay_postcommit_read");
    let batch = ProjectionReplayBatch {
        projection: projection.clone(),
        source_identity: "orders".to_owned(),
        batch_id: "batch-1".to_owned(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: "event-1".to_owned(),
            checkpoint: "checkpoint-1".to_owned(),
            position: Some(1),
            document_id: "doc-1".to_owned(),
            payload: Some(serde_json::json!({"title": "committed"})),
        }],
    };
    super::super::replay::inject_replay_postcommit_metadata_read_failure();

    // Act
    let failed = cassie.replay_projection_batch(batch.clone());
    let row = cassie.midge.get_document(&projection, "doc-1").unwrap();
    let event = cassie
        .midge
        .has_projection_event(&projection, "orders", "event-1")
        .unwrap();
    let checkpoint = cassie
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap()
        .source_checkpoint;
    let checkpoint_query = format!("SELECT source_checkpoint FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'");
    let catalog_before_retry = execute(&cassie, &session, &checkpoint_query);
    let retry = cassie.replay_projection_batch(batch).unwrap();
    let catalog_after_retry = execute(&cassie, &session, &checkpoint_query);
    let row_after_retry = cassie.midge.get_document(&projection, "doc-1").unwrap();
    let index = cassie
        .midge
        .get_index(&projection, "replay_postcommit_title_idx")
        .unwrap()
        .unwrap();
    let controls = cassie.runtime.query_controls(std::time::Instant::now());
    let index_request = crate::midge::adapter::ScalarIndexScanRequest {
        equality_prefix: vec![serde_json::json!("committed")],
        ..Default::default()
    };
    let index_hits = cassie
        .midge
        .scan_scalar_index_controlled(&index, &index_request, &controls)
        .unwrap();
    let indexed_after_retry = execute(
        &cassie,
        &session,
        "SELECT title FROM replay_postcommit_read WHERE title = 'committed'",
    );

    // Assert
    assert!(failed.is_err());
    assert_eq!(
        row.unwrap().payload,
        serde_json::json!({"title": "committed"})
    );
    assert!(event);
    assert_eq!(checkpoint.as_deref(), Some("checkpoint-1"));
    assert_eq!(
        catalog_before_retry,
        vec![vec![Value::String(String::new())]]
    );
    assert_eq!(
        catalog_after_retry,
        vec![vec![Value::String("checkpoint-1".to_owned())]]
    );
    assert_eq!(
        row_after_retry.unwrap().payload,
        serde_json::json!({"title": "committed"})
    );
    assert_eq!(
        indexed_after_retry,
        vec![vec![Value::String("committed".to_owned())]]
    );
    assert_eq!(index_hits.as_slice().len(), 1);
    assert_eq!(index_hits.as_slice()[0].id, "doc-1");
    assert_eq!(retry.applied_event_count, 0);
    assert_eq!(retry.skipped_duplicate_count, 1);
}

#[test]
fn should_reconcile_aborted_native_unknown_after_restart() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-replay-aborted-unknown-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE replay_aborted_unknown (title TEXT)",
    );
    let projection = cassie
        .midge
        .canonical_collection_name("replay_aborted_unknown");
    let batch = ProjectionReplayBatch {
        projection: projection.clone(),
        source_identity: "orders".to_owned(),
        batch_id: "batch-1".to_owned(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: "event-1".to_owned(),
            checkpoint: "checkpoint-1".to_owned(),
            position: Some(1),
            document_id: "doc-1".to_owned(),
            payload: Some(serde_json::json!({"title": "retried"})),
        }],
    };
    crate::midge::adapter::inject_native_commit_unknown_before_commit();

    // Act
    let failed = cassie.replay_projection_batch(batch.clone());
    let fenced = cassie.midge.projection_metadata(&projection);
    cassie.shutdown();
    drop(cassie);
    let recovered =
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default()).unwrap();
    recovered.startup().unwrap();
    let before = recovered
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap();
    let missing_row = recovered.midge.get_document(&projection, "doc-1").unwrap();
    let missing_event = recovered
        .midge
        .has_projection_event(&projection, "orders", "event-1")
        .unwrap();
    let applied = recovered.replay_projection_batch(batch.clone()).unwrap();
    let duplicate = recovered.replay_projection_batch(batch).unwrap();
    let row = recovered.midge.get_document(&projection, "doc-1").unwrap();

    // Assert
    assert!(failed.is_err());
    assert!(fenced.is_err());
    assert!(missing_row.is_none());
    assert!(!missing_event);
    assert_eq!(before.source_checkpoint, None);
    assert_eq!(applied.applied_event_count, 1);
    assert_eq!(duplicate.applied_event_count, 0);
    assert_eq!(duplicate.skipped_duplicate_count, 1);
    assert_eq!(
        row.unwrap().payload,
        serde_json::json!({"title": "retried"})
    );
    recovered.shutdown();
    drop(recovered);
    std::fs::remove_dir_all(path).unwrap();
}
