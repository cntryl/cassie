#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::sql::ast::{AlterMaterializedProjectionOperation, QueryStatement};
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_parse_materialized_projection_lifecycle_commands() {
    // Arrange
    let create_sql = "CREATE MATERIALIZED PROJECTION projection_ready AS SELECT title FROM docs";
    let activate_sql = "ALTER MATERIALIZED PROJECTION projection_ready ACTIVATE VERSION v2 UNSAFE";
    let drop_version_sql = "DROP MATERIALIZED PROJECTION VERSION projection_ready VERSION v1";

    // Act
    let create = cassie::sql::parse_statement(create_sql).unwrap();
    let activate = cassie::sql::parse_statement(activate_sql).unwrap();
    let drop_version = cassie::sql::parse_statement(drop_version_sql).unwrap();

    // Assert
    let QueryStatement::CreateMaterializedProjection(create) = create.statement else {
        panic!("expected CREATE MATERIALIZED PROJECTION");
    };
    assert_eq!(create.name, "projection_ready");
    assert_eq!(create.query, "SELECT title FROM docs");

    let QueryStatement::AlterMaterializedProjection(activate) = activate.statement else {
        panic!("expected ALTER MATERIALIZED PROJECTION");
    };
    match activate.operation {
        AlterMaterializedProjectionOperation::ActivateVersion {
            version_id,
            unsafe_override,
        } => {
            assert_eq!(version_id, "v2");
            assert!(unsafe_override);
        }
        AlterMaterializedProjectionOperation::BuildVersion => panic!("expected activate version"),
    }

    let QueryStatement::DropMaterializedProjectionVersion(drop_version) = drop_version.statement
    else {
        panic!("expected DROP MATERIALIZED PROJECTION VERSION");
    };
    assert_eq!(drop_version.name, "projection_ready");
    assert_eq!(drop_version.version_id, "v1");
}

#[test]
fn should_replay_projection_batch_idempotently_with_checkpoint_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_replay_idempotent");
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
                "CREATE TABLE projection_replay_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        let batch = ProjectionReplayBatch {
            projection: "projection_replay_docs".to_string(),
            source_identity: "orders-stream".to_string(),
            batch_id: "batch-1".to_string(),
            lag: 0,
            events: vec![ProjectionReplayEvent {
                event_id: "event-1".to_string(),
                checkpoint: "checkpoint-1".to_string(),
                position: Some(1),
                document_id: "doc-1".to_string(),
                payload: Some(serde_json::json!({"title": "alpha"})),
            }],
        };

        // Act
        let first = cassie.replay_projection_batch(batch.clone()).unwrap();
        let duplicate = cassie.replay_projection_batch(batch).unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                "SELECT source_identity, source_checkpoint, last_applied_event_id, replay_batch_id, freshness FROM pg_catalog.pg_projection_checkpoints WHERE collection = 'projection_replay_docs'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(first.applied_event_count, 1);
        assert_eq!(duplicate.skipped_duplicate_count, 1);
        assert_eq!(selected.rows, vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(
            checkpoint.rows,
            vec![vec![
                Value::String("orders-stream".to_string()),
                Value::String("checkpoint-1".to_string()),
                Value::String("event-1".to_string()),
                Value::String("batch-1".to_string()),
                Value::String("fresh".to_string()),
            ]]
        );

        let metrics = cassie.metrics();
        assert_eq!(metrics["projections"]["replay_batches"].as_u64(), Some(2));
        assert_eq!(
            metrics["projections"]["replay_duplicates_skipped"].as_u64(),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_skip_existing_projection_events_while_applying_new_replay_events() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_replay_mixed_duplicate_batch");
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
                "CREATE TABLE projection_replay_mixed_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: "projection_replay_mixed_docs".to_string(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-1".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-1".to_string(),
                    checkpoint: "checkpoint-1".to_string(),
                    position: Some(1),
                    document_id: "doc-1".to_string(),
                    payload: Some(serde_json::json!({"title": "alpha"})),
                }],
            })
            .unwrap();

        // Act
        let replay = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: "projection_replay_mixed_docs".to_string(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-2".to_string(),
                lag: 0,
                events: vec![
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-1".to_string(),
                        position: Some(1),
                        document_id: "doc-1".to_string(),
                        payload: Some(serde_json::json!({"title": "alpha-updated"})),
                    },
                    ProjectionReplayEvent {
                        event_id: "event-2".to_string(),
                        checkpoint: "checkpoint-2".to_string(),
                        position: Some(2),
                        document_id: "doc-2".to_string(),
                        payload: Some(serde_json::json!({"title": "bravo"})),
                    },
                ],
            })
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_mixed_docs ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(replay.applied_event_count, 1);
        assert_eq!(replay.skipped_duplicate_count, 1);
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("bravo".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_failed_freshness_for_out_of_order_replay() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_replay_out_of_order");
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
                "CREATE TABLE projection_replay_order_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: "projection_replay_order_docs".to_string(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-1".to_string(),
                lag: 0,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-2".to_string(),
                    checkpoint: "checkpoint-2".to_string(),
                    position: Some(2),
                    document_id: "doc-2".to_string(),
                    payload: Some(serde_json::json!({"title": "bravo"})),
                }],
            })
            .unwrap();

        // Act
        let error = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: "projection_replay_order_docs".to_string(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-2".to_string(),
                lag: 1,
                events: vec![ProjectionReplayEvent {
                    event_id: "event-1".to_string(),
                    checkpoint: "checkpoint-1".to_string(),
                    position: Some(1),
                    document_id: "doc-1".to_string(),
                    payload: Some(serde_json::json!({"title": "alpha"})),
                }],
            })
            .unwrap_err();
        let checkpoint = cassie
            .execute_sql(
                &session,
                "SELECT freshness, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = 'projection_replay_order_docs'",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(error.to_string().contains("out-of-order"));
        assert_eq!(checkpoint.rows.len(), 1);
        assert_eq!(checkpoint.rows[0][0], Value::String("failed".to_string()));
        assert!(matches!(&checkpoint.rows[0][1], Value::String(message) if message.contains("out-of-order")));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fail_duplicate_event_id_in_batch_without_partial_replay() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_replay_duplicate_in_batch");
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
                "CREATE TABLE projection_replay_conflict_docs (title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .replay_projection_batch(ProjectionReplayBatch {
                projection: "projection_replay_conflict_docs".to_string(),
                source_identity: "orders-stream".to_string(),
                batch_id: "batch-conflict".to_string(),
                lag: 2,
                events: vec![
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-1".to_string(),
                        position: Some(1),
                        document_id: "doc-1".to_string(),
                        payload: Some(serde_json::json!({"title": "alpha"})),
                    },
                    ProjectionReplayEvent {
                        event_id: "event-1".to_string(),
                        checkpoint: "checkpoint-2".to_string(),
                        position: Some(2),
                        document_id: "doc-2".to_string(),
                        payload: Some(serde_json::json!({"title": "bravo"})),
                    },
                ],
            })
            .unwrap_err();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_replay_conflict_docs ORDER BY title",
                vec![],
            )
            .unwrap();
        let checkpoint = cassie
            .execute_sql(
                &session,
                "SELECT freshness, replay_batch_id, source_checkpoint, last_applied_event_id, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = 'projection_replay_conflict_docs'",
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let restarted_checkpoint = restarted
            .execute_sql(
                &restarted_session,
                "SELECT freshness, replay_batch_id, source_checkpoint, last_applied_event_id, last_error FROM pg_catalog.pg_projection_checkpoints WHERE collection = 'projection_replay_conflict_docs'",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(error.to_string().contains("duplicate projection replay event"));
        assert_eq!(selected.rows, Vec::<Vec<Value>>::new());
        assert_eq!(checkpoint.rows.len(), 1);
        assert_eq!(checkpoint.rows[0][0], Value::String("failed".to_string()));
        assert_eq!(
            checkpoint.rows[0][1],
            Value::String("batch-conflict".to_string())
        );
        assert_eq!(checkpoint.rows[0][2], Value::String(String::new()));
        assert_eq!(checkpoint.rows[0][3], Value::String(String::new()));
        assert!(matches!(&checkpoint.rows[0][4], Value::String(message) if message.contains("duplicate projection replay event")));
        assert_eq!(restarted_checkpoint.rows, checkpoint.rows);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_refresh_materialized_projection_after_source_write() {
    // Arrange
    with_fallback();
    let path = data_dir("materialized_projection_refresh");
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
                "CREATE TABLE projection_source_docs (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title, score) VALUES ('alpha', 1), ('bravo', 2)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_ready AS SELECT title, score FROM projection_source_docs WHERE score > 1",
                vec![],
            )
            .unwrap();
        let initial = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM projection_ready ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_source_docs (title, score) VALUES ('charlie', 3)",
                vec![],
            )
            .unwrap();
        let stale = cassie
            .execute_sql(
                &session,
                "SELECT state FROM pg_catalog.pg_materialized_projections WHERE projection_name = 'projection_ready'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "REFRESH MATERIALIZED PROJECTION projection_ready",
                vec![],
            )
            .unwrap();
        let refreshed = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_ready ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            initial.rows,
            vec![vec![Value::String("bravo".to_string()), Value::Int64(2)]]
        );
        assert_eq!(stale.rows, vec![vec![Value::String("stale".to_string())]]);
        assert_eq!(
            refreshed.rows,
            vec![
                vec![Value::String("bravo".to_string())],
                vec![Value::String("charlie".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_dml_against_materialized_projection_output() {
    // Arrange
    with_fallback();
    let path = data_dir("materialized_projection_read_only");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE TABLE projection_ro_docs (title TEXT)", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_ro_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_ro AS SELECT title FROM projection_ro_docs",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_ro (title) VALUES ('bravo')",
                vec![],
            )
            .unwrap_err();

        // Assert
        assert!(error.to_string().contains("read-only"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_activate_built_materialized_projection_version() {
    // Arrange
    with_fallback();
    let path = data_dir("materialized_projection_versions");
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
                "CREATE TABLE projection_version_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_version_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_versioned AS SELECT title FROM projection_version_docs",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_version_docs (title) VALUES ('bravo')",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED PROJECTION projection_versioned BUILD VERSION",
                vec![],
            )
            .unwrap();
        let before_swap = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_versioned ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER MATERIALIZED PROJECTION projection_versioned ACTIVATE VERSION v2",
                vec![],
            )
            .unwrap();
        let after_swap = cassie
            .execute_sql(
                &session,
                "SELECT title FROM projection_versioned ORDER BY title",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DROP MATERIALIZED PROJECTION VERSION projection_versioned VERSION v1",
                vec![],
            )
            .unwrap();
        let versions = cassie
            .execute_sql(
                &session,
                "SELECT version_id, state FROM pg_catalog.pg_projection_versions WHERE projection_name = 'projection_versioned' ORDER BY version_id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            before_swap.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );
        assert_eq!(
            after_swap.rows,
            vec![
                vec![Value::String("alpha".to_string())],
                vec![Value::String("bravo".to_string())],
            ]
        );
        assert_eq!(
            versions.rows,
            vec![vec![
                Value::String("v2".to_string()),
                Value::String("active".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_materialized_projection_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("materialized_projection_restart");
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
                "CREATE TABLE projection_restart_docs (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO projection_restart_docs (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION projection_restart AS SELECT title FROM projection_restart_docs",
                vec![],
            )
            .unwrap();
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT title FROM projection_restart ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
