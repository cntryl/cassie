use super::{data_dir, with_fallback};
use cassie::app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::catalog::ProjectionVerificationState;

fn projection_metric_delta(
    after: &serde_json::Value,
    before: &serde_json::Value,
    key: &str,
) -> u64 {
    after["projections"][key]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(before["projections"][key].as_u64().unwrap_or_default())
}

#[test]
fn should_record_projection_replay_write_amplification() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_replay_write_amplification");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_replay_metrics_docs (title TEXT)",
                vec![],
            )
            .unwrap();

        let before = cassie.metrics();
        let events = vec![
            ProjectionReplayEvent {
                event_id: "replay-write-amplification-1".to_string(),
                checkpoint: "checkpoint-1".to_string(),
                position: Some(1),
                document_id: "doc-1".to_string(),
                payload: Some(serde_json::json!({"title": "alpha"})),
            },
            ProjectionReplayEvent {
                event_id: "replay-write-amplification-2".to_string(),
                checkpoint: "checkpoint-2".to_string(),
                position: Some(2),
                document_id: "doc-2".to_string(),
                payload: Some(serde_json::json!({"title": "bravo"})),
            },
        ];
        let batch = ProjectionReplayBatch {
            projection: "projection_replay_metrics_docs".to_string(),
            source_identity: "replay-metrics-stream".to_string(),
            batch_id: "replay-metrics-batch".to_string(),
            lag: 0,
            events,
        };

        // Act
        let report = cassie.replay_projection_batch(batch).unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(report.applied_event_count, 2);
        assert_eq!(
            projection_metric_delta(&after, &before, "write_row_puts"),
            2
        );
        assert_eq!(
            projection_metric_delta(&after, &before, "write_metadata_puts"),
            2
        );
        assert_eq!(
            projection_metric_delta(&after, &before, "write_batch_flushes"),
            1
        );
        assert_eq!(
            projection_metric_delta(&after, &before, "replay_events_applied"),
            2
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_duplicate_replay_checks_without_row_puts() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_replay_duplicate_checks");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_replay_duplicate_docs (title TEXT)",
                vec![],
            )
            .unwrap();

        let first = ProjectionReplayBatch {
            projection: "projection_replay_duplicate_docs".to_string(),
            source_identity: "replay-dup-stream".to_string(),
            batch_id: "replay-dup-first".to_string(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "replay-dup-event".to_string(),
                checkpoint: "checkpoint-dup-1".to_string(),
                position: Some(1),
                document_id: "dup-doc".to_string(),
                payload: Some(serde_json::json!({"title": "first"})),
            }],
        };
        cassie.replay_projection_batch(first).unwrap();

        let before = cassie.metrics();
        let second = ProjectionReplayBatch {
            projection: "projection_replay_duplicate_docs".to_string(),
            source_identity: "replay-dup-stream".to_string(),
            batch_id: "replay-dup-second".to_string(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "replay-dup-event".to_string(),
                checkpoint: "checkpoint-dup-2".to_string(),
                position: Some(2),
                document_id: "dup-doc".to_string(),
                payload: Some(serde_json::json!({"title": "replacement"})),
            }],
        };

        // Act
        let report = cassie.replay_projection_batch(second).unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(report.applied_event_count, 0);
        assert_eq!(report.skipped_duplicate_count, 1);
        assert_eq!(
            projection_metric_delta(&after, &before, "write_row_puts"),
            0
        );
        assert_eq!(
            projection_metric_delta(&after, &before, "write_row_deletes"),
            0
        );
        assert_eq!(
            projection_metric_delta(&after, &before, "write_duplicate_checks"),
            1
        );
        assert_eq!(
            projection_metric_delta(&after, &before, "replay_duplicates_skipped"),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_projection_rebuild_write_categories() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_rebuild_write_categories");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_rebuild_source_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_source_docs (title, score) VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .unwrap();

        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_rebuild_metric_projection AS SELECT title, score FROM projection_rebuild_source_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_source_docs (title, score) VALUES ('charlie', 3)",
                vec![],
            )
            .unwrap();
        let after_create = cassie.metrics();

        // Act
        cassie
            .execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_rebuild_metric_projection",
                vec![],
            )
            .unwrap();
        let after_refresh = cassie.metrics();

        // Assert
        assert_eq!(
            projection_metric_delta(&after_refresh, &after_create, "write_rebuild_target_puts"),
            3
        );
        assert_eq!(
            projection_metric_delta(&after_refresh, &after_create, "write_batch_flushes"),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_leave_projection_rebuild_hashes_current_after_refresh() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_rebuild_current_hashes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_rebuild_hash_source (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_hash_source (title, score) VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_rebuild_hash_projection AS SELECT title, score FROM projection_rebuild_hash_source ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_rebuild_hash_source (title, score) VALUES ('charlie', 3)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_rebuild_hash_projection",
                vec![],
            )
            .unwrap();
        let metadata = cassie
            .catalog
            .get_materialized_projection("projection_rebuild_hash_projection")
            .unwrap();
        let verification = cassie
            .execute_sql(
                &session,
                "VERIFY PROJECTION projection_rebuild_hash_projection MODE full",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            metadata.hashes.root.state,
            ProjectionVerificationState::Current
        );
        assert_eq!(metadata.hashes.root.row_count, 3);
        assert_eq!(metadata.verification.state, ProjectionVerificationState::Verified);
        assert_eq!(
            verification.rows[0][0],
            cassie::types::Value::String("verified".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_projection_activation_metadata_write() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_activation_metadata_write");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_activation_source_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_activation_metric_projection AS SELECT title, score FROM projection_activation_source_docs",
                vec![],
            )
            .unwrap();

        let before = cassie.metrics();

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED PROJECTION projection_activation_metric_projection BUILD VERSION",
                vec![],
            )
            .unwrap();
        let version_id = cassie
            .catalog
            .get_materialized_projection("projection_activation_metric_projection")
            .and_then(|metadata| {
                metadata
                    .versions
                    .last()
                    .map(|version| version.version_id.clone())
            })
            .unwrap_or_else(|| "v1".to_string());

        cassie
            .execute_sql(
                &session,
                &format!(
                    "ALTER MATERIALIZED PROJECTION projection_activation_metric_projection ACTIVATE VERSION {version_id}"
                ),
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            projection_metric_delta(&after, &before, "write_activation_metadata_writes"),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
