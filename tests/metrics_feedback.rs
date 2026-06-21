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
fn should_capture_runtime_feedback_for_normalized_select() {
    // Arrange
    with_fallback();
    let path = data_dir("feedback_capture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_feedback_capture";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_capture WHERE title = $1";
        let key = feedback_key(sql, collection, 0);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let metrics = cassie.metrics();
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("scan feedback should be recorded");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(record.executions, 1);
        assert_eq!(record.rows_out_total, 1);
        assert_eq!(record.errors_total, 0);
        assert!(
            metrics["feedback"]["writes"].as_u64().unwrap_or_default() >= 1,
            "feedback writes should be tracked"
        );
        assert!(
            metrics["feedback"]["misses"].as_u64().unwrap_or_default() >= 1,
            "first feedback lookup should miss"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_aggregate_runtime_feedback_across_parameter_values() {
    // Arrange
    with_fallback();
    let path = data_dir("feedback_aggregate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_feedback_aggregate";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_aggregate WHERE title = $1";
        let key = feedback_key(sql, collection, 0);

        // Act
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .unwrap();
        let metrics = cassie.metrics();
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("scan feedback should aggregate");

        // Assert
        assert_eq!(record.executions, 2);
        assert_eq!(record.rows_out_total, 2);
        assert!(
            metrics["feedback"]["hits"].as_u64().unwrap_or_default() >= 1,
            "second feedback lookup should hit"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_partition_runtime_feedback_by_schema_epoch() {
    // Arrange
    with_fallback();
    let path = data_dir("feedback_schema_epoch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_feedback_schema_epoch";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_schema_epoch WHERE title = $1";
        let first_key = feedback_key(sql, collection, 0);
        let second_key = feedback_key(sql, collection, 1);

        // Act
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE feedback_schema_marker (id INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .unwrap();
        let first = cassie
            .feedback_record_for_diagnostics(&first_key)
            .expect("first schema epoch feedback");
        let second = cassie
            .feedback_record_for_diagnostics(&second_key)
            .expect("second schema epoch feedback");

        // Assert
        assert_eq!(first.executions, 1);
        assert_eq!(second.executions, 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_evict_runtime_feedback_when_retention_limit_is_exceeded() {
    // Arrange
    with_fallback();
    let path = data_dir("feedback_eviction");
    let mut config = cassie::config::CassieRuntimeConfig::from_env();
    config.limits.feedback_entries = 2;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_feedback_eviction";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let first_sql = "SELECT title FROM metrics_feedback_eviction";
        let second_sql = "SELECT body FROM metrics_feedback_eviction";
        let first_key = feedback_key(first_sql, collection, 0);

        // Act
        cassie.execute_sql(&session, first_sql, vec![]).unwrap();
        cassie.execute_sql(&session, second_sql, vec![]).unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert!(cassie.feedback_record_for_diagnostics(&first_key).is_none());
        assert_eq!(metrics["feedback"]["entries"].as_u64(), Some(2));
        assert!(
            metrics["feedback"]["evictions"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "retention limit should evict the oldest feedback"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_runtime_feedback_in_explain_analyze_output() {
    // Arrange
    with_fallback();
    let path = data_dir("feedback_explain_analyze");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_feedback_explain_analyze";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);

        // Act
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN ANALYZE SELECT title FROM metrics_feedback_explain_analyze WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();
        let metrics = cassie.metrics();

        // Assert
        assert!(plan.contains("analyze=true"), "plan={plan}");
        assert!(plan.contains("operator_actuals=Scan:"), "plan={plan}");
        assert!(plan.contains("rows_out:1"), "plan={plan}");
        assert!(
            metrics["feedback"]["writes"].as_u64().unwrap_or_default() >= 1,
            "EXPLAIN ANALYZE should write feedback"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_runtime_feedback_for_candidate_budget() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_feedback");
    let config = adaptive_candidate_config(1, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_feedback";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_feedback ORDER BY score DESC LIMIT 1";
        let wider_sql = "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_feedback ORDER BY score DESC LIMIT 2";

        cassie.execute_sql(&session, sql, vec![]).unwrap();
        let seeded = cassie.metrics();

        // Act
        cassie.execute_sql(&session, wider_sql, vec![]).unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            after["adaptive_candidates"]["initial_budget_total"]
                .as_u64()
                .unwrap_or_default()
                - seeded["adaptive_candidates"]["initial_budget_total"]
                    .as_u64()
                    .unwrap_or_default(),
            3
        );
        assert_eq!(
            after["adaptive_candidates"]["feedback_budget_total"]
                .as_u64()
                .unwrap_or_default()
                - seeded["adaptive_candidates"]["feedback_budget_total"]
                    .as_u64()
                    .unwrap_or_default(),
            3
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
