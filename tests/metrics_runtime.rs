#![allow(unused_imports, dead_code)]
use cassie::app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::catalog::{IndexKind, IndexMeta, ProjectionVerificationState};
use cassie::runtime::RuntimeFeedbackKey;
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-metrics-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn startup_frame(user: &str, database: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);
    payload.push(0);

    let mut frame = Vec::new();
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

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

fn feedback_key(sql: &str, collection: &str, schema_epoch: u64) -> RuntimeFeedbackKey {
    let _ = (sql, collection, schema_epoch);
    panic!("feedback_key helper is unused in metrics_runtime");
}

fn register_feedback_collection(cassie: &Cassie, collection: &str) {
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(collection, schema);
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha", "body": "one"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-2".to_string()),
            serde_json::json!({"title": "beta", "body": "two"}),
        )
        .unwrap();
}

fn adaptive_candidate_config(min: usize, max: usize) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.adaptive_candidate_min = min;
    config.limits.adaptive_candidate_max = max;
    config
}

fn register_adaptive_candidate_collection(cassie: &Cassie, collection: &str) {
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "body".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(collection, schema);
    for (id, body) in [
        ("doc-1", "alpha shared"),
        ("doc-2", "alpha shared"),
        ("doc-3", "alpha shared"),
    ] {
        cassie
            .midge
            .put_document(
                collection,
                Some(id.to_string()),
                serde_json::json!({"body": body}),
            )
            .unwrap();
    }
}

fn describe_statement_frame(statement_name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(b'S');
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'D');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("describe payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn read_auth_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, i32, Vec<u8>) {
    let mut header = [0u8; 5];
    tokio::io::AsyncReadExt::read_exact(reader, &mut header)
        .await
        .expect("read auth frame header");

    let tag = header[0];
    let len = i32::from_be_bytes(header[1..].try_into().expect("auth frame length"));
    let mut payload =
        vec![0u8; usize::try_from(len - 4).expect("non-negative auth payload length")];
    tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
        .await
        .expect("read auth frame payload");

    (tag, len, payload)
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, Vec<u8>) {
    let mut tag = [0u8; 1];
    tokio::io::AsyncReadExt::read_exact(reader, &mut tag)
        .await
        .expect("read frame tag");

    let mut len = [0u8; 4];
    tokio::io::AsyncReadExt::read_exact(reader, &mut len)
        .await
        .expect("read frame length");
    let len = i32::from_be_bytes(len);
    let mut payload = vec![0u8; usize::try_from(len - 4).expect("non-negative payload length")];
    if !payload.is_empty() {
        tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
            .await
            .expect("read frame payload");
    }

    (tag[0], payload)
}

#[test]
fn should_report_runtime_metrics_snapshot() {
    // Arrange
    with_fallback();
    let path = data_dir("startup_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "metrics_runtime_docs";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(collection, schema.clone());
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_runtime_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_runtime_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["ready"], serde_json::Value::Bool(true));
        assert!(
            metrics["runtime"]["startup_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "startup counter should be recorded"
        );
        assert!(
            metrics["runtime"]["catalog_hydration_total"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "catalog hydration counter should be recorded"
        );
        assert_eq!(metrics["query"]["count"].as_u64(), Some(2));
        assert_eq!(metrics["query"]["rows_returned_total"].as_u64(), Some(2));
        assert!(
            metrics["storage"]["schema"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "schema storage reads should be recorded"
        );
        assert!(
            metrics["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "data storage reads should be recorded"
        );
        assert!(
            metrics["storage"]["temp"]["writes"]
                .as_u64()
                .unwrap_or_default()
                > 0,
            "temp storage writes should be recorded"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_read_path_metrics_for_point_lookup_collection_scan() {
    // Arrange
    with_fallback();
    let path = data_dir("read_path_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_read_paths";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(collection, schema);
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha", "status": "active"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"title": "bravo", "status": "queued"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();
        let before_point_hits = before["read_paths"]["point_lookup_hits"]
            .as_u64()
            .unwrap_or_default();
        let before_point_misses = before["read_paths"]["point_lookup_misses"]
            .as_u64()
            .unwrap_or_default();
        let before_point_scans = before["read_paths"]["point_lookup_scans"]
            .as_u64()
            .unwrap_or_default();
        let before_scans = before["read_paths"]["collection_scans"]
            .as_u64()
            .unwrap_or_default();

        // Act
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_read_paths WHERE id = 'doc-1'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_read_paths WHERE id = 'missing'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(&session, "SELECT title FROM metrics_read_paths", vec![])
            .unwrap();

        // Assert
        let after = cassie.metrics();
        assert_eq!(
            after["read_paths"]["point_lookup_scans"]
                .as_u64()
                .unwrap_or_default(),
            before_point_scans + 2,
        );
        assert_eq!(
            after["read_paths"]["point_lookup_hits"]
                .as_u64()
                .unwrap_or_default(),
            before_point_hits + 1,
        );
        assert_eq!(
            after["read_paths"]["point_lookup_misses"]
                .as_u64()
                .unwrap_or_default(),
            before_point_misses + 1,
        );
        assert_eq!(
            after["read_paths"]["collection_scans"]
                .as_u64()
                .unwrap_or_default(),
            before_scans + 1,
        );
        assert!(after["read_paths"]["last_point_lookup_collection"]
            .as_str()
            .is_some());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_expose_cardinality_metrics_with_explain_plan_estimates() {
    // Arrange
    with_fallback();
    let path = data_dir("cardinality_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_cardinality_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha", "body": "bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_index(IndexMeta {
                collection: collection.to_string(),
                name: "idx_title".to_string(),
                field: "title".to_string(),
                fields: vec!["title".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::Scalar,
                unique: false,
                options: Default::default(),
            })
            .unwrap();
        cassie.midge.delete_cardinality_stats(collection).unwrap();

        // Act
        cassie.startup().unwrap();
        cassie
            .ingest_document(
                collection,
                serde_json::json!({"title": "beta", "body": "charlie"}),
            )
            .unwrap();

        let session = cassie.create_session("tester", None);
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM metrics_cardinality_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();
        let metrics = cassie.metrics();

        // Assert
        assert!(plan.contains("estimates=scan:2"), "plan={plan}");
        assert!(plan.contains("index:1"), "plan={plan}");
        assert!(plan.contains("cost_source=advanced_stats"), "plan={plan}");
        assert!(
            metrics["cardinality"]["reads"].as_u64().unwrap_or_default() >= 1,
            "cardinality reads should be tracked"
        );
        assert!(
            metrics["cardinality"]["writes"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "cardinality writes should be tracked"
        );
        assert!(
            metrics["cardinality"]["rebuilds"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "cardinality rebuilds should be tracked"
        );
        assert!(
            metrics["cardinality"]["unavailable"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "missing stats should be tracked"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_query_error_statistics() {
    // Arrange
    with_fallback();
    let path = data_dir("query_errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();
        let before_count = before["query"]["count"].as_u64().unwrap_or_default();
        let before_errors = before["query"]["errors_total"].as_u64().unwrap_or_default();

        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT title FROM metrics_missing_query_errors",
            vec![],
        );
        let after = cassie.metrics();

        // Assert
        assert!(result.is_err(), "missing collection should fail");
        assert_eq!(
            after["query"]["count"].as_u64().unwrap_or_default() - before_count,
            1
        );
        assert_eq!(
            after["query"]["errors_total"].as_u64().unwrap_or_default() - before_errors,
            1
        );
        assert!(after["query"]["errors_by_class"]
            .as_object()
            .expect("errors by class")
            .values()
            .any(|count| count.as_u64().unwrap_or_default() > 0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_count_failed_scan_as_storage_read_error() {
    // Arrange
    with_fallback();
    let path = data_dir("scan_errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.catalog.register_collection(
            "missing_storage_collection",
            vec![("title".to_string(), DataType::Text)],
        );

        let before = cassie.metrics();
        let before_errors = before["storage"]["data"]["errors"]
            .as_u64()
            .unwrap_or_default();
        let before_reads = before["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();

        let session = cassie.create_session("tester", None);
        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT title FROM missing_storage_collection WHERE title = 'alpha'",
            vec![],
        );
        assert!(
            result.is_err(),
            "query should fail because collection schema is missing in storage"
        );

        let after = cassie.metrics();

        // Assert
        assert_eq!(
            after["storage"]["data"]["errors"]
                .as_u64()
                .unwrap_or_default()
                - before_errors,
            1
        );
        assert!(
            after["storage"]["data"]["reads"]
                .as_u64()
                .unwrap_or_default()
                > before_reads,
            "scan failure should still record the read attempt"
        );

        let _ = std::fs::remove_dir_all(path);
    });
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
