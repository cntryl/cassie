use crate::app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::observe_native_commits;

fn fixture() -> (Cassie, String) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE replay_policy_docs (title TEXT)",
            vec![],
        )
        .unwrap();
    let projection = cassie.midge.canonical_collection_name("replay_policy_docs");
    (cassie, projection)
}

fn batch(projection: &str) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_owned(),
        source_identity: "stream".to_owned(),
        batch_id: "batch-1".to_owned(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: "event-1".to_owned(),
            checkpoint: "checkpoint-1".to_owned(),
            position: Some(1),
            document_id: "doc-1".to_owned(),
            payload: Some(serde_json::json!({"title": "alpha"})),
        }],
    }
}

#[test]
fn should_use_buffered_wal_for_default_replay() {
    // Arrange
    let (cassie, projection) = fixture();

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.replay_projection_batch(batch(&projection)));

    // Assert
    assert_eq!(result.unwrap().applied_event_count, 1);
    assert!(writes.local_buffered > 0);
    assert_eq!(writes.best_effort, 0);
    assert_eq!(
        cassie
            .midge
            .projection_metadata(&projection)
            .unwrap()
            .unwrap()
            .source_checkpoint
            .as_deref(),
        Some("checkpoint-1")
    );
}

#[test]
fn should_use_synchronous_wal_for_strict_replay() {
    // Arrange
    let (cassie, projection) = fixture();

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.replay_projection_batch_strict(batch(&projection)));

    // Assert
    assert_eq!(result.unwrap().applied_event_count, 1);
    assert!(writes.local_sync > 0);
    assert_eq!(writes.local_buffered, 0);
    assert_eq!(writes.best_effort, 0);
    assert_eq!(
        cassie
            .midge
            .projection_metadata(&projection)
            .unwrap()
            .unwrap()
            .source_checkpoint
            .as_deref(),
        Some("checkpoint-1")
    );
}

#[test]
fn should_use_replayable_policy_for_metadata_only_batches() {
    // Arrange
    let (cassie, projection) = fixture();
    cassie
        .replay_projection_batch_strict(batch(&projection))
        .unwrap();
    let mut empty = batch(&projection);
    empty.batch_id = "empty".to_owned();
    empty.events.clear();

    // Act
    let (empty_result, empty_writes) =
        observe_native_commits(|| cassie.replay_projection_batch(empty));
    let (duplicate_result, duplicate_writes) =
        observe_native_commits(|| cassie.replay_projection_batch(batch(&projection)));

    // Assert
    assert_eq!(empty_result.unwrap().applied_event_count, 0);
    assert_eq!(duplicate_result.unwrap().skipped_duplicate_count, 1);
    assert!(empty_writes.local_buffered > 0);
    assert!(duplicate_writes.local_buffered > 0);
    assert_eq!(empty_writes.best_effort, 0);
    assert_eq!(duplicate_writes.best_effort, 0);
}

#[test]
fn should_use_strict_policy_for_metadata_only_batches() {
    // Arrange
    let (cassie, projection) = fixture();
    cassie
        .replay_projection_batch_strict(batch(&projection))
        .unwrap();
    let mut empty = batch(&projection);
    empty.batch_id = "empty".to_owned();
    empty.events.clear();

    // Act
    let (empty_result, empty_writes) =
        observe_native_commits(|| cassie.replay_projection_batch_strict(empty));
    let (duplicate_result, duplicate_writes) =
        observe_native_commits(|| cassie.replay_projection_batch_strict(batch(&projection)));

    // Assert
    assert_eq!(empty_result.unwrap().applied_event_count, 0);
    assert_eq!(duplicate_result.unwrap().skipped_duplicate_count, 1);
    assert!(empty_writes.local_sync > 0);
    assert!(duplicate_writes.local_sync > 0);
    assert_eq!(empty_writes.local_buffered, 0);
    assert_eq!(duplicate_writes.local_buffered, 0);
}
