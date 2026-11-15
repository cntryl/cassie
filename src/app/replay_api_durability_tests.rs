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
    assert_eq!(writes.first_policy, Some("local_buffered"));
    assert!(writes.local_buffered_attempts > 0);
    assert_eq!(writes.best_effort_attempts, 0);
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
    assert_eq!(writes.first_policy, Some("local_sync"));
    assert!(writes.local_sync_attempts > 0);
    assert_eq!(writes.local_buffered_attempts, 0);
    assert_eq!(writes.best_effort_attempts, 0);
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
    assert_eq!(empty_writes.first_policy, Some("local_buffered"));
    assert_eq!(duplicate_writes.first_policy, Some("local_buffered"));
    assert!(empty_writes.local_buffered_attempts > 0);
    assert!(duplicate_writes.local_buffered_attempts > 0);
    assert_eq!(empty_writes.best_effort_attempts, 0);
    assert_eq!(duplicate_writes.best_effort_attempts, 0);
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
    assert_eq!(empty_writes.first_policy, Some("local_sync"));
    assert_eq!(duplicate_writes.first_policy, Some("local_sync"));
    assert!(empty_writes.local_sync_attempts > 0);
    assert!(duplicate_writes.local_sync_attempts > 0);
    assert_eq!(empty_writes.local_buffered_attempts, 0);
    assert_eq!(duplicate_writes.local_buffered_attempts, 0);
}

fn delete_missing(projection: &str) -> ProjectionReplayBatch {
    let mut batch = batch(projection);
    "batch-2".clone_into(&mut batch.batch_id);
    "event-2".clone_into(&mut batch.events[0].event_id);
    "checkpoint-2".clone_into(&mut batch.events[0].checkpoint);
    batch.events[0].position = Some(2);
    "missing".clone_into(&mut batch.events[0].document_id);
    batch.events[0].payload = None;
    batch
}

#[test]
fn should_use_replayable_policy_for_missing_row_deletion() {
    // Arrange
    let (cassie, projection) = fixture();
    cassie
        .replay_projection_batch_strict(batch(&projection))
        .unwrap();

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.replay_projection_batch(delete_missing(&projection)));

    // Assert
    assert_eq!(result.unwrap().applied_event_count, 1);
    assert_eq!(writes.first_policy, Some("local_buffered"));
    assert_eq!(writes.best_effort_attempts, 0);
    assert_eq!(
        cassie
            .midge
            .projection_metadata(&projection)
            .unwrap()
            .unwrap()
            .source_checkpoint
            .as_deref(),
        Some("checkpoint-2")
    );
}

#[test]
fn should_use_strict_policy_for_missing_row_deletion() {
    // Arrange
    let (cassie, projection) = fixture();
    cassie
        .replay_projection_batch_strict(batch(&projection))
        .unwrap();

    // Act
    let (result, writes) = observe_native_commits(|| {
        cassie.replay_projection_batch_strict(delete_missing(&projection))
    });

    // Assert
    assert_eq!(result.unwrap().applied_event_count, 1);
    assert_eq!(writes.first_policy, Some("local_sync"));
    assert_eq!(writes.best_effort_attempts, 0);
    assert_eq!(
        cassie
            .midge
            .projection_metadata(&projection)
            .unwrap()
            .unwrap()
            .source_checkpoint
            .as_deref(),
        Some("checkpoint-2")
    );
}

fn cloud_policy_case(
    configured_strict: bool,
    explicit_strict: bool,
) -> (u64, Option<&'static str>, u64) {
    let path = std::env::temp_dir().join(format!(
        "cassie-replay-cloud-policy-{}",
        uuid::Uuid::new_v4()
    ));
    let config = CassieRuntimeConfig::default();
    let midge = crate::midge::adapter::Midge::new_cloud_simulated_for_test(
        &path,
        &config.database,
        configured_strict,
    )
    .unwrap();
    let cassie = Cassie::new_with_midge_and_config(std::sync::Arc::new(midge), config).unwrap();
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
    let (result, writes) = observe_native_commits(|| {
        if explicit_strict {
            cassie.replay_projection_batch_strict(batch(&projection))
        } else {
            cassie.replay_projection_batch(batch(&projection))
        }
    });
    let applied = result.unwrap().applied_event_count;
    cassie.shutdown();
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).unwrap();
    (applied, writes.first_policy, writes.best_effort_attempts)
}

#[test]
fn should_apply_selected_cloud_policy_to_replay_commit() {
    // Arrange
    let cases = [(false, false), (false, true), (true, false), (true, true)];

    // Act
    let observed = cases.map(|(configured, explicit)| cloud_policy_case(configured, explicit));

    // Assert
    assert_eq!(
        observed,
        [
            (1, Some("cloud_async"), 0),
            (1, Some("cloud_strict"), 0),
            (1, Some("cloud_strict"), 0),
            (1, Some("cloud_strict"), 0),
        ]
    );
}

#[test]
fn should_attribute_every_replay_acknowledgement_commit_to_one_source() {
    // Arrange
    let (cassie, projection) = fixture();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX replay_policy_title_idx ON replay_policy_docs USING btree (title)",
            vec![],
        )
        .unwrap();

    // Act
    let (result, writes) =
        observe_native_commits(|| cassie.replay_projection_batch(batch(&projection)));

    // Assert
    result.unwrap();
    let maintenance = writes.column_maintenance_attempts
        + writes.hash_maintenance_attempts
        + writes.time_series_maintenance_attempts;
    assert_eq!(writes.first_policy, Some("local_buffered"));
    assert!(writes.primary_attempts >= 1);
    assert_eq!(writes.attempted, writes.primary_attempts + maintenance);
    assert!(writes.maintenance_barrier_attempts <= maintenance);
}
