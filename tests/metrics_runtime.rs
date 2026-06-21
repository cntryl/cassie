#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
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

fn feedback_key(sql: &str, collection: &str, schema_epoch: u64) -> RuntimeFeedbackKey {
    let parsed = parser::parse_statement(sql).expect("parse feedback sql");
    RuntimeFeedbackKey {
        sql_fingerprint: cassie::runtime::sql_fingerprint(&parsed),
        schema_epoch,
        database: Some("postgres".to_string()),
        collection: collection.to_string(),
        operator: "Scan".to_string(),
    }
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
    let mut config = cassie::config::CassieRuntimeConfig::from_env();
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
        assert!(plan.contains("index:2"), "plan={plan}");
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
