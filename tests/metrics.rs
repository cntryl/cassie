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
                include_fields: Vec::new(),
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
fn should_record_vector_counts_for_ordered_search_expression() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_vector_candidates";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())

            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "embedding": [1.0, 0.0],
                }),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({
                    "title": "beta",
                    "embedding": [0.0, 1.0],
                }),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({
                    "title": "gamma",
                    "embedding": [1.0, 1.0],
                }),
            )

            .unwrap();

        let before = cassie.metrics();
        let before_candidates = before["vector"]["candidate_count_total"].as_u64().unwrap_or_default();
        let before_results = before["vector"]["result_count_total"].as_u64().unwrap_or_default();

        let session = cassie.create_session("tester", None);
        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_vector_candidates ORDER BY embedding <-> '[1,0]' LIMIT 1",
                vec![],
            )

.unwrap();

        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["vector"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            3
        );
        assert_eq!(
            after["vector"]["result_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_results,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_vector_prefilter_candidate_counts() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_prefilter_counts");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_vector_prefilter_counts";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "status".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(2),
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({
                    "status": "approved",
                    "embedding": [1.0, 0.0],
                }),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({
                    "status": "approved",
                    "embedding": [2.0, 0.0],
                }),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({
                    "status": "pending",
                    "embedding": [3.0, 0.0],
                }),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_input = before["vector"]["prefilter_input_candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_filtered = before["vector"]["prefilter_filtered_candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_fallback = before["vector"]["prefilter_fallback_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM metrics_vector_prefilter_counts WHERE status = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["vector"]["prefilter_input_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_input,
            3
        );
        assert_eq!(
            after["vector"]["prefilter_filtered_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_filtered,
            2
        );
        assert_eq!(
            after["vector"]["prefilter_fallback_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_fallback,
            0
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_search_operator_statistics() {
    // Arrange
    with_fallback();
    let path = data_dir("search_operator_stats");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_search_operator_stats";
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
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"body": "alpha bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"body": "alpha charlie"}),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_candidates = before["search"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_results = before["search"]["result_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_search_operator_stats ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["search"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            2
        );
        assert_eq!(
            after["search"]["result_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_results,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_search_operator_candidates_after_posting_list_filtering() {
    // Arrange
    with_fallback();
    let path = data_dir("search_operator_posting_list_candidates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_search_operator_posting_list_candidates";
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
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"body": "alpha bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"body": "bravo charlie"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-3".to_string()),
                serde_json::json!({"body": "charlie delta"}),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_candidates = before["search"]["candidate_count_total"]
            .as_u64()
            .unwrap_or_default();
        let before_results = before["search"]["result_count_total"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_search_operator_posting_list_candidates WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["search"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_candidates,
            1
        );
        assert_eq!(
            after["search"]["result_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before_results,
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_adaptive_candidate_expansion() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_expansion");
    let config = adaptive_candidate_config(1, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_expansion";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_expansion ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            after["adaptive_candidates"]["decisions"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["decisions"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["initial_budget_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["initial_budget_total"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["final_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["final_candidate_count_total"]
                    .as_u64()
                    .unwrap_or_default(),
            3
        );
        assert!(
            after["adaptive_candidates"]["expansions_total"]
                .as_u64()
                .unwrap_or_default()
                > before["adaptive_candidates"]["expansions_total"]
                    .as_u64()
                    .unwrap_or_default(),
            "candidate work beyond the initial budget should be counted"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_adaptive_candidate_cap_overflow() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_cap");
    let config = adaptive_candidate_config(1, 1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_cap";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_cap ORDER BY score DESC LIMIT 2",
                vec![],
            )
            .expect_err("query should exceed adaptive candidate cap");
        let after = cassie.metrics();

        // Assert
        assert!(
            error
                .to_string()
                .contains("adaptive candidate max"),
            "error should name the adaptive candidate cap: {error}"
        );
        assert_eq!(
            after["adaptive_candidates"]["limit_errors_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["limit_errors_total"]
                    .as_u64()
                    .unwrap_or_default(),
            1
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

#[test]
fn should_report_adaptive_candidate_budget_in_explain() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_explain");
    let config = adaptive_candidate_config(2, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_explain";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_explain ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = result.rows[0][0].as_str().unwrap_or_default();
        assert!(
            plan.contains("candidate_budget=2"),
            "explain should include adaptive candidate budget: {plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_candidate_tie_order() {
    // Arrange
    with_fallback();
    let path = data_dir("adaptive_candidate_ties");
    let config = adaptive_candidate_config(1, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        let collection = "metrics_adaptive_candidate_ties";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_ties ORDER BY score DESC LIMIT 3",
                vec![],
            )
            .unwrap();

        // Assert
        let ids = result
            .rows
            .iter()
            .map(|row| row[0].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["doc-1", "doc-2", "doc-3"]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cache_fulltext_scoring_metadata_for_repeated_search_queries() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_scoring_metadata_cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_fulltext_scoring_metadata_cache";
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
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"body": "alpha bravo"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-2".to_string()),
                serde_json::json!({"body": "alpha charlie"}),
            )
            .unwrap();

        let before = cassie.metrics();
        let before_hits = before["query_cache"]["fulltext_stats_hits"]
            .as_u64()
            .unwrap_or_default();
        let before_misses = before["query_cache"]["fulltext_stats_misses"]
            .as_u64()
            .unwrap_or_default();
        let session = cassie.create_session("tester", None);

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_fulltext_scoring_metadata_cache WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after_first = cassie.metrics();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_fulltext_scoring_metadata_cache WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let after_second = cassie.metrics();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO metrics_fulltext_scoring_metadata_cache (body) VALUES ('alpha delta')",
                vec![],
            )
            .unwrap();
        let third = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_fulltext_scoring_metadata_cache WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after_third = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 2);
        assert_eq!(third.rows.len(), 1);
        assert_eq!(
            after_first["query_cache"]["fulltext_stats_misses"]
                .as_u64()
                .unwrap_or_default()
                - before_misses,
            1
        );
        assert_eq!(
            after_first["query_cache"]["fulltext_stats_hits"]
                .as_u64()
                .unwrap_or_default()
                - before_hits,
            0
        );
        assert_eq!(
            after_second["query_cache"]["fulltext_stats_hits"]
                .as_u64()
                .unwrap_or_default()
                - before_hits,
            1
        );
        assert_eq!(
            after_second["query_cache"]["fulltext_stats_misses"]
                .as_u64()
                .unwrap_or_default()
                - before_misses,
            1
        );
        assert_eq!(
            after_third["query_cache"]["fulltext_stats_hits"]
                .as_u64()
                .unwrap_or_default()
                - before_hits,
            1
        );
        assert_eq!(
            after_third["query_cache"]["fulltext_stats_misses"]
                .as_u64()
                .unwrap_or_default()
                - before_misses,
            2
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
fn should_report_plan_cache_metrics() {
    // Arrange
    with_fallback();
    let path = data_dir("plan_cache_metrics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_plan_cache_docs";
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
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
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
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_plan_cache_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM metrics_plan_cache_docs WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(first.rows.len(), 1);
        assert_eq!(second.rows.len(), 1);
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));
        assert!(
            metrics["plan_cache"]["entries"]
                .as_u64()
                .unwrap_or_default()
                >= 1
        );

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
fn should_track_protocol_errors_for_missing_prepared_statement_describe() {
    // Arrange
    with_fallback();
    let path = data_dir("pgwire_protocol_errors");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let before_protocol_errors = cassie.metrics()["pgwire"]["protocol_errors_total"]
            .as_u64()
            .unwrap_or_default();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let mut config = cassie::config::CassieRuntimeConfig::from_env();
        config.password.clear();
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        let startup = startup_frame("postgres", "testdb");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
            .await
            .expect("startup write");

        let auth_frame = read_auth_frame(&mut reader).await;
        assert_eq!(
            auth_frame.0, b'R',
            "startup should return an authentication response"
        );
        let startup_ready = read_wire_frame(&mut reader).await;
        assert_eq!(startup_ready.0, b'Z', "startup should end ready-for-query");
        assert_eq!(startup_ready.1, vec![b'I']);

        // Act
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &describe_statement_frame("missing"))
            .await
            .expect("describe write");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush");
        let response = read_wire_frame(&mut reader).await;
        assert_eq!(response.0, b'E', "describe should return an error frame");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        drop(socket);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            metrics["pgwire"]["protocol_errors_total"]
                .as_u64()
                .unwrap_or_default()
                - before_protocol_errors,
            1,
            "missing describe statement should count as a protocol error"
        );

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
