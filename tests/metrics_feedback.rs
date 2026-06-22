#![allow(unused_imports, dead_code)]
use cassie::app::{Cassie, CassieSession};
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::runtime::{RuntimeFeedbackKey, RuntimeFeedbackObservation};
use cassie::types::{DataType, FieldSchema, Schema};
use std::time::Duration;
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

fn operator_feedback_config(enabled: bool) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env();
    config.limits.operator_feedback_enabled = enabled;
    config
}

fn feedback_key(
    cassie: &Cassie,
    session: &CassieSession,
    sql: &str,
    candidate_index: Option<&str>,
) -> RuntimeFeedbackKey {
    cassie
        .read_operator_feedback_key_for_diagnostics(session, sql, candidate_index)
        .expect("feedback key")
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

fn register_operator_feedback_indexes(
    cassie: &Cassie,
    collection: &str,
    first_index: &str,
    second_index: &str,
) {
    for (field, index_name) in [("body", first_index), ("title", second_index)] {
        let index = IndexMeta {
            collection: collection.to_string(),
            name: index_name.to_string(),
            field: field.to_string(),
            fields: vec![field.to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: Default::default(),
        };
        cassie.midge.put_index(index.clone()).unwrap();
        cassie.catalog.register_index(index);
    }
}

fn confident_feedback(elapsed_ms: u64, storage_reads: u64) -> RuntimeFeedbackObservation {
    RuntimeFeedbackObservation {
        rows_in: storage_reads.max(1),
        rows_out: 1,
        elapsed_ms,
        storage_reads,
        ..RuntimeFeedbackObservation::default()
    }
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
        let key = feedback_key(&cassie, &session, sql, None);

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
fn should_capture_runtime_feedback_for_selected_index() {
    // Arrange
    with_fallback();
    let path = data_dir("feedback_selected_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_feedback_selected_index";
        let index_name = "metrics_feedback_selected_title_idx";
        register_feedback_collection(&cassie, collection);
        let index = IndexMeta {
            collection: collection.to_string(),
            name: index_name.to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: Default::default(),
        };
        cassie.midge.put_index(index.clone()).unwrap();
        cassie.catalog.register_index(index);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT body FROM metrics_feedback_selected_index WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, Some(index_name));

        // Act
        let result = cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM metrics_feedback_selected_index WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("selected index feedback should be recorded");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(record.executions, 1);
        assert_eq!(record.rows_out_total, 1);
        assert_eq!(key.operator_family, "index_read");
        let cassie::types::Value::String(plan) = &explained.rows[0][0] else {
            panic!("expected explain string");
        };
        assert!(plan.contains("index_feedback=enabled"));
        assert!(plan.contains(index_name));

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
        let key = feedback_key(&cassie, &session, sql, None);

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
        let first_key = feedback_key(&cassie, &session, sql, None);

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
        let second_key = feedback_key(&cassie, &session, sql, None);
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .unwrap();
        let first = cassie.feedback_record_for_diagnostics(&first_key).is_none();
        let second = cassie
            .feedback_record_for_diagnostics(&second_key)
            .expect("second schema epoch feedback");

        // Assert
        assert!(first, "schema changes should invalidate older feedback");
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
    config.limits.feedback_entries = 1;
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
        let first_key = feedback_key(&cassie, &session, first_sql, None);

        // Act
        cassie.execute_sql(&session, first_sql, vec![]).unwrap();
        cassie.execute_sql(&session, second_sql, vec![]).unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert!(cassie.feedback_record_for_diagnostics(&first_key).is_none());
        assert_eq!(metrics["feedback"]["entries"].as_u64(), Some(1));
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
fn should_ignore_operator_feedback_when_confidence_is_too_low() {
    // Arrange
    with_fallback();
    let path = data_dir("operator_feedback_low_confidence");
    let config = operator_feedback_config(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_operator_feedback_low_confidence";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_operator_feedback_low_confidence WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, None);

        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();

        // Act
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM metrics_operator_feedback_low_confidence WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("low-confidence feedback");

        // Assert
        assert!(record.confidence_bps < 600, "record={record:?}");
        assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
        assert!(
            plan.contains("operator_feedback_reason=low_confidence"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_confident_operator_feedback_to_switch_selected_index() {
    // Arrange
    with_fallback();
    let path = data_dir("operator_feedback_switch");
    let config = operator_feedback_config(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_operator_feedback_switch";
        let base_index = "metrics_operator_feedback_body_idx_a";
        let preferred_index = "metrics_operator_feedback_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let explain_sql = "EXPLAIN SELECT title FROM metrics_operator_feedback_switch WHERE title = 'alpha' AND body = 'one'";
        let shape_sql = "SELECT title FROM metrics_operator_feedback_switch WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));

        let baseline = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let baseline_plan = baseline.rows[0][0].as_str().unwrap().to_string();
        assert!(baseline_plan.contains(base_index), "plan={baseline_plan}");

        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(base_key.clone(), confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(preferred_key.clone(), confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }
        let base_record = cassie
            .feedback_record_for_diagnostics(&base_key)
            .expect("base record");
        let preferred_record = cassie
            .feedback_record_for_diagnostics(&preferred_key)
            .expect("preferred record");
        assert_ne!(base_key, preferred_key);
        assert!(
            preferred_record.stable_average_elapsed_ms()
                < base_record.stable_average_elapsed_ms()
        );

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();

        // Assert
        assert!(plan.contains(preferred_index), "plan={plan}");
        assert!(plan.contains("operator_feedback=used"), "plan={plan}");
        assert!(
            plan.contains(&format!(
                "operator_feedback_base_candidate=index:{base_index}"
            )),
            "plan={plan}"
        );
        assert!(
            plan.contains(&format!(
                "operator_feedback_selected_candidate=index:{preferred_index}"
            )),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_to_base_index_when_operator_feedback_is_disabled() {
    // Arrange
    with_fallback();
    let path = data_dir("operator_feedback_disabled");
    let config = operator_feedback_config(false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_operator_feedback_disabled";
        let base_index = "metrics_operator_feedback_disabled_body_idx_a";
        let preferred_index = "metrics_operator_feedback_disabled_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let explain_sql = "EXPLAIN SELECT title FROM metrics_operator_feedback_disabled WHERE title = 'alpha' AND body = 'one'";
        let shape_sql = "SELECT title FROM metrics_operator_feedback_disabled WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));

        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(base_key.clone(), confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(preferred_key.clone(), confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();

        // Assert
        assert!(plan.contains(base_index), "plan={plan}");
        assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
        assert!(
            plan.contains("operator_feedback_reason=disabled"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_stale_operator_feedback_in_explain_diagnostics() {
    // Arrange
    with_fallback();
    let path = data_dir("operator_feedback_stale");
    let mut config = operator_feedback_config(true);
    config.limits.feedback_ttl_seconds = 1;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_operator_feedback_stale";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_operator_feedback_stale WHERE title = $1";

        for value in ["alpha", "beta", "alpha", "beta"] {
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String(value.to_string())],
                )
                .unwrap();
        }
        std::thread::sleep(Duration::from_millis(1_200));

        // Act
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM metrics_operator_feedback_stale WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let plan = explain.rows[0][0].as_str().unwrap().to_string();

        // Assert
        assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
        assert!(
            plan.contains("operator_feedback_reason=stale"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_persisted_operator_feedback_from_storage() {
    // Arrange
    with_fallback();
    let path = data_dir("operator_feedback_restart");
    let config = operator_feedback_config(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_operator_feedback_restart (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for (title, body) in [("alpha", "one"), ("beta", "two"), ("gamma", "three")] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO metrics_operator_feedback_restart (title, body) VALUES ($1, $2)",
                    vec![
                        cassie::types::Value::String(title.to_string()),
                        cassie::types::Value::String(body.to_string()),
                    ],
                )
                .unwrap();
        }
        let sql = "SELECT title FROM metrics_operator_feedback_restart WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, None);

        for value in ["alpha", "beta", "gamma"] {
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String(value.to_string())],
                )
                .unwrap();
        }
        assert!(
            !cassie
                .midge
                .list_runtime_feedback_records()
                .unwrap()
                .is_empty(),
            "feedback records should be persisted into storage"
        );
        cassie.clear_feedback_for_diagnostics();
        assert!(
            cassie.feedback_record_for_diagnostics(&key).is_none(),
            "clearing runtime feedback should remove the in-memory record"
        );

        cassie
            .reload_feedback_from_storage_for_diagnostics()
            .expect("reload feedback from storage");

        // Act
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("persisted feedback record");

        // Assert
        assert_eq!(record.executions, 3, "record={record:?}");
        assert!(record.confidence_bps >= 600, "record={record:?}");

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
