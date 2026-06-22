#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::runtime::RuntimeFeedbackObservation;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;

fn adaptive_execution_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::default();
    config.limits.operator_feedback_enabled = true;
    config.limits.adaptive_execution_enabled = true;
    config.limits.adaptive_min_cost_savings_bps = 0;
    config
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

#[test]
fn should_order_column_top_k_with_deterministic_tie_break() {
    // Arrange
    with_fallback();
    let path = data_dir("column_top_k_tie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let collection = "sql_column_top_k_tie";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
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
                Some("d2".to_string()),
                serde_json::json!({"title": "second", "score": 10}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d1".to_string()),
                serde_json::json!({"title": "first", "score": 10}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "third", "score": 1}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM sql_column_top_k_tie ORDER BY score DESC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_results_for_adaptive_read_operator_choice() {
    // Arrange
    with_fallback();
    let fixed_path = data_dir("adaptive_read_operator_fixed");
    let adaptive_path = data_dir("adaptive_read_operator_enabled");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let fixed = Cassie::new_with_data_dir(&fixed_path).unwrap();
        let adaptive =
            Cassie::new_with_data_dir_and_config(&adaptive_path, adaptive_execution_config())
                .unwrap();
        let fixed_session = fixed.create_session("tester", None);
        let adaptive_session = adaptive.create_session("tester", None);
        for cassie in [&fixed, &adaptive] {
            let session = if std::ptr::eq(cassie, &fixed) {
                &fixed_session
            } else {
                &adaptive_session
            };
            cassie
                .execute_sql(
                    session,
                    "CREATE TABLE sql_adaptive_projection (title TEXT, body TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    "CREATE INDEX sql_adaptive_projection_body_idx_a ON sql_adaptive_projection (body)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    "CREATE INDEX sql_adaptive_projection_title_idx_b ON sql_adaptive_projection (title)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    session,
                    "INSERT INTO sql_adaptive_projection (title, body) VALUES ('alpha', 'one'), ('beta', 'two')",
                    vec![],
                )
                .unwrap();
        }

        let sql = "SELECT title FROM sql_adaptive_projection WHERE title = 'alpha' AND body = 'one'";
        let base_index = "sql_adaptive_projection_body_idx_a";
        let preferred_index = "sql_adaptive_projection_title_idx_b";
        let base_key = adaptive
            .read_operator_feedback_key_for_diagnostics(&adaptive_session, sql, Some(base_index))
            .unwrap();
        let preferred_key = adaptive
            .read_operator_feedback_key_for_diagnostics(
                &adaptive_session,
                sql,
                Some(preferred_index),
            )
            .unwrap();
        for _ in 0..4 {
            adaptive
                .seed_feedback_for_diagnostics(base_key.clone(), confident_feedback(90, 24))
                .unwrap();
            adaptive
                .seed_feedback_for_diagnostics(preferred_key.clone(), confident_feedback(5, 1))
                .unwrap();
        }

        // Act
        let fixed_result = fixed.execute_sql(&fixed_session, sql, vec![]).unwrap();
        let adaptive_result = adaptive
            .execute_sql(&adaptive_session, sql, vec![])
            .unwrap();
        let adaptive_plan = adaptive
            .execute_sql(&adaptive_session, &format!("EXPLAIN {sql}"), vec![])
            .unwrap();
        let plan = adaptive_plan.rows[0][0].as_str().unwrap_or_default();

        // Assert
        assert_eq!(fixed_result.rows, adaptive_result.rows);
        assert_eq!(
            adaptive_result.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );
        assert!(plan.contains(preferred_index), "plan={plan}");
        assert!(
            plan.contains("adaptive_reason=selected_operator_feedback"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(fixed_path);
        let _ = std::fs::remove_dir_all(adaptive_path);
    });
}

#[test]
fn should_fall_back_for_filtered_ordered_column_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("column_top_k_filter_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let collection = "sql_column_top_k_filter_fallback";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
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
                Some("d1".to_string()),
                serde_json::json!({"title": "skip", "score": 100}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "keep", "score": 10}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM sql_column_top_k_filter_fallback WHERE title = 'keep' ORDER BY score DESC LIMIT 1",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d2".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_function_projection_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("projected_scan_function_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_function_fallback";
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
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT upper(title) FROM sql_projected_scan_function_fallback WHERE title = 'alpha'",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("ALPHA".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_wildcard_projection_query_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("projected_scan_wildcard_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_projected_scan_wildcard_fallback";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
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
                Some("d1".to_string()),
                serde_json::json!({"title": "alpha", "score": 7}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT * FROM sql_projected_scan_wildcard_fallback WHERE score = 7",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(result.rows[0][2], Value::Int64(7));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_describe_select_projection_with_column_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("describe_sql_metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "sql_describe_metadata";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
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

        // Act
        let columns = cassie
            .describe_sql("SELECT id, title, score FROM sql_describe_metadata")
            .unwrap();

        // Assert
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0].name, "id");
        assert_eq!(columns[0].type_oid, DataType::Text.type_oid());
        assert_eq!(columns[1].name, "title");
        assert_eq!(columns[1].data_type, "text");
        assert_eq!(columns[2].name, "score");
        assert_eq!(columns[2].type_oid, DataType::Int.type_oid());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_projected_crud_queries_against_column_store_tables() {
    // Arrange
    with_fallback();
    let path = data_dir("column_store_projection_crud");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::default();
        config.limits.experimental_column_store_enabled = true;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        let collection = "sql_column_store_projection_crud";

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_column_store_projection_crud (doc_id TEXT, title TEXT, summary TEXT, score INT) WITH (storage = column_store)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_column_store_projection_crud (doc_id, title, summary, score) VALUES ('d1', 'alpha', NULL, 10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_column_store_projection_crud (doc_id, title, score) VALUES ('d2', 'beta', 20)",
                vec![],
            )
            .unwrap();

        // Act
        let before = cassie
            .execute_sql(
                &session,
                "SELECT doc_id, title, summary, score FROM sql_column_store_projection_crud ORDER BY doc_id",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title, score FROM sql_column_store_projection_crud WHERE doc_id = 'd2'",
                vec![],
            )
            .unwrap();
        let documents = cassie.midge.scan_documents(collection).unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE sql_column_store_projection_crud SET summary = 'filled' WHERE doc_id = 'd2'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM sql_column_store_projection_crud WHERE doc_id = 'd1'",
                vec![],
            )
            .unwrap();
        let after = cassie
            .execute_sql(
                &session,
                "SELECT doc_id, title, summary, score FROM sql_column_store_projection_crud ORDER BY doc_id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            before.rows,
            vec![
                vec![
                    Value::String("d1".to_string()),
                    Value::String("alpha".to_string()),
                    Value::Null,
                    Value::Int64(10),
                ],
                vec![
                    Value::String("d2".to_string()),
                    Value::String("beta".to_string()),
                    Value::Null,
                    Value::Int64(20),
                ],
            ]
        );
        let explicit_null = documents
            .iter()
            .find(|document| {
                document.payload.get("doc_id") == Some(&serde_json::Value::String("d1".to_string()))
            })
            .expect("stored null row");
        let missing = documents
            .iter()
            .find(|document| {
                document.payload.get("doc_id") == Some(&serde_json::Value::String("d2".to_string()))
            })
            .expect("stored missing row");
        assert!(matches!(
            explicit_null.payload.get("summary"),
            Some(serde_json::Value::Null)
        ));
        assert!(missing.payload.get("summary").is_none());

        let plan = explain_plan_text(&explain);
        assert_explain_contains(plan, "storage_mode", "column-store");

        assert_eq!(
            after.rows,
            vec![vec![
                Value::String("d2".to_string()),
                Value::String("beta".to_string()),
                Value::String("filled".to_string()),
                Value::Int64(20),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_column_store_creation_when_experimental_mode_is_disabled() {
    // Arrange
    with_fallback();
    let path = data_dir("column_store_disabled");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_column_store_disabled (title TEXT) WITH (storage = column_store)",
                vec![],
            )
            .expect_err("column-store creation should be gated");

        // Assert
        assert!(error.to_string().contains("column-store"));
        assert!(!cassie.catalog.exists("sql_column_store_disabled"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_column_store_schema_rewrites_before_partial_write() {
    // Arrange
    with_fallback();
    let path = data_dir("column_store_schema_rewrite");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::default();
        config.limits.experimental_column_store_enabled = true;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_column_store_schema_rewrite (title TEXT) WITH (storage = column_store)",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "ALTER TABLE sql_column_store_schema_rewrite ADD COLUMN summary TEXT",
                vec![],
            )
            .expect_err("column-store schema rewrite should fail");
        let columns = cassie.describe_sql("SELECT title FROM sql_column_store_schema_rewrite").unwrap();

        // Assert
        assert!(error.to_string().contains("column-store"));
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].name, "title");

        let _ = std::fs::remove_dir_all(path);
    });
}
