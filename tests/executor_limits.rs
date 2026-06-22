#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
use cassie::executor;
use cassie::planner::logical::LogicalPlan;
use cassie::planner::physical::PhysicalPlan;
use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
use cassie::sql::binder;
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

#[path = "support/executor.rs"]
mod support;
use support::*;

#[tokio::test]
async fn should_fail_query_when_query_timeout_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.query_timeout_ms = 0;
    let path = data_dir("timeout");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_timeout";
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
    let result = cassie.execute_sql(&session, "SELECT title FROM exec_timeout", vec![]);

    // Assert
    let message = result
        .expect_err("query should fail when timeout is configured to 0")
        .to_string();
    assert!(
        message.contains("query timeout exceeded"),
        "expected timeout error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_fail_query_when_result_limit_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.max_result_rows = 1;
    let path = data_dir("max_rows");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_max_rows";
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
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-2".to_string()),
            serde_json::json!({"title": "beta"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "SELECT title FROM exec_max_rows ORDER BY title",
        vec![],
    );

    // Assert
    let message = result
        .expect_err("query should fail when row limit is configured too low")
        .to_string();
    assert!(
        message.contains("query result row limit exceeded"),
        "expected row limit error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_fail_query_when_cte_recursion_depth_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.cte_recursion_depth = 0;
    let path = data_dir("cte_depth");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_cte_depth";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "n".to_string(),
            data_type: DataType::Int,
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
            Some("d1".to_string()),
            serde_json::json!({"n": 1}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE seq(n) AS (SELECT n FROM exec_cte_depth WHERE n = 1 UNION ALL SELECT n FROM seq WHERE n = 1) SELECT n FROM seq",
                vec![],
            )
            ;

    // Assert
    let message = result
        .expect_err("recursive cte should fail when depth is exhausted")
        .to_string();
    assert!(
        message.contains("did not stabilize within 0 iterations"),
        "expected recursion depth error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn should_fail_query_when_temporary_spill_budget_is_exceeded() {
    // Arrange
    with_fallback();
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.temp_spill_budget_bytes = 16;
    let path = data_dir("spill_budget");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();

    let collection = "exec_spill";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "payload".to_string(),
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
            serde_json::json!({"payload": "very long payload data for spill budget test"}),
        )
        .unwrap();

    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(&session, "SELECT payload FROM exec_spill", vec![]);

    // Assert
    let message = result
        .expect_err("query should fail when temp spill budget is exhausted")
        .to_string();
    assert!(
        message.contains("temporary storage budget exceeded"),
        "expected spill budget error, got {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_skip_offset_then_take_limit() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_offset_limit";

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
                Some("d1".to_string()),
                serde_json::json!({"title": "a"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "b"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "c"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d4".to_string()),
                serde_json::json!({"title": "d"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d5".to_string()),
                serde_json::json!({"title": "e"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_offset_limit ORDER BY title ASC LIMIT 2 OFFSET 2",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 2);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["d3".to_string(), "d4".to_string()]);
    });
}

#[test]
fn should_default_missing_offset_to_zero_in_execution() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_default_offset";

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
                Some("d1".to_string()),
                serde_json::json!({"title": "c"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "a"}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d3".to_string()),
                serde_json::json!({"title": "b"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let default_offset_result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1",
                vec![],
            )
            .expect("query should execute");

        let explicit_offset_result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_default_offset ORDER BY title ASC LIMIT 1 OFFSET 0",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(default_offset_result.rows.len(), 1);
        assert_eq!(explicit_offset_result.rows.len(), 1);
        assert_eq!(default_offset_result.rows, explicit_offset_result.rows);
    });
}

#[test]
fn should_cleanup_parallel_aggregation_workers_on_timeout() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_aggregation_timeout");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_aggregation_workers = 4;
    config.limits.query_timeout_ms = 0;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_timeout";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "category".to_string(),
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
        let documents = (0..1024)
            .map(|index| {
                (
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({
                        "category": format!("g{}", index % 4),
                        "score": 1,
                    }),
                )
            })
            .collect::<Vec<_>>();
        cassie.midge.put_documents(collection, documents).unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie.execute_sql(
            &session,
            "SELECT category, SUM(score) FROM exec_parallel_aggregation_timeout GROUP BY category",
            vec![],
        );
        let metrics = cassie.metrics();

        // Assert
        let message = result
            .expect_err("parallel aggregate should time out")
            .to_string();
        assert!(
            message.contains("query timeout exceeded"),
            "expected timeout error, got {message}"
        );
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
    });

    let _ = std::fs::remove_dir_all(path);
}
