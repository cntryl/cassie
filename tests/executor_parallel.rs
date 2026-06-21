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

const PARALLEL_ROW_COUNT: usize = 1025;

fn create_registered_collection(cassie: &Cassie, collection: &str, fields: &[(&str, DataType)]) {
    let schema = Schema {
        fields: fields
            .iter()
            .map(|(name, data_type)| FieldSchema {
                name: (*name).to_string(),
                data_type: data_type.clone(),
                nullable: true,
            })
            .collect(),
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
}

fn put_documents(
    cassie: &Cassie,
    collection: &str,
    documents: impl IntoIterator<Item = (String, serde_json::Value)>,
) {
    cassie
        .midge
        .put_documents(
            collection,
            documents
                .into_iter()
                .map(|(id, payload)| (Some(id), payload))
                .collect(),
        )
        .unwrap();
}

#[test]
fn should_score_fulltext_candidates_with_parallel_workers() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_scoring_fulltext");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_scoring_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_scoring_fulltext";
        let session = cassie.create_session("tester", None);
        create_registered_collection(
            &cassie,
            collection,
            &[("id", DataType::Text), ("body", DataType::Text)],
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "id": format!("doc-{index:04}"),
                        "body": "alpha beta",
                    }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM exec_parallel_scoring_fulltext WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 3",
                vec![],
            )
            .expect("parallel scoring query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 3);
        assert!(metrics["parallel_scoring"]["scorings"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert!(metrics["parallel_scoring"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2);
        assert!(metrics["parallel_scoring"]["rows"].as_u64().unwrap_or(0) > 0);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_scoring_when_worker_limit_is_one() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_scoring_fallback");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_scoring_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_scoring_fallback (id TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_parallel_scoring_fallback (id, body) VALUES ('doc-1', 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM exec_parallel_scoring_fallback WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .expect("fallback scoring query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(metrics["parallel_scoring"]["scorings"].as_u64(), Some(0));
        assert!(metrics["parallel_scoring"]["fallback_scorings"]
            .as_u64()
            .unwrap_or(0)
            > 0);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_merge_parallel_scan_batches_deterministically() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_scan_merge");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_scan_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_scan_merge";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
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
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "id": format!("doc-{index:04}"),
                        "title": format!("title-{index:04}"),
                    }),
                )
            }),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT * FROM exec_parallel_scan_merge", vec![])
            .expect("parallel scan query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), PARALLEL_ROW_COUNT);
        assert!(metrics["parallel_scans"]["scans"].as_u64().unwrap_or(0) > 0);
        assert!(metrics["parallel_scans"]["workers"].as_u64().unwrap_or(0) >= 2);
        assert_eq!(
            metrics["parallel_scans"]["rows"].as_u64(),
            Some(PARALLEL_ROW_COUNT as u64)
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_scan_when_worker_limit_is_one() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_scan_single_worker");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_scan_single_worker";
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
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM exec_parallel_scan_single_worker",
                vec![],
            )
            .expect("single-worker query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::String("alpha".to_string())]]);
        assert_eq!(metrics["parallel_scans"]["scans"].as_u64(), Some(0));
        assert!(
            metrics["parallel_scans"]["fallback_scans"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_grouped_aggregates_with_parallel_workers() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_aggregation_grouped");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_grouped";
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
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = if index % 2 == 0 { "even" } else { "odd" };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "category": category,
                        "score": 1,
                    }),
                )
            }),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total, SUM(score) AS sum_score FROM exec_parallel_aggregation_grouped GROUP BY category ORDER BY category",
                vec![],
            )
            .expect("parallel aggregate query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    Value::String("even".to_string()),
                    Value::Int64(513),
                    Value::Int64(513),
                ],
                vec![
                    Value::String("odd".to_string()),
                    Value::Int64(512),
                    Value::Int64(512),
                ],
            ]
        );
        assert!(metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert!(metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2);
        assert_eq!(
            metrics["parallel_aggregation"]["rows"].as_u64(),
            Some(PARALLEL_ROW_COUNT as u64)
        );
        assert_eq!(metrics["parallel_aggregation"]["groups"].as_u64(), Some(2));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_ungrouped_parallel_aggregates_with_nulls() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_aggregation_ungrouped_nulls");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_nulls";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("score", DataType::Int)]);
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let score = if index % 10 == 0 {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(1)
                };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({ "score": score }),
                )
            }),
        );
        let non_null_rows = (0..PARALLEL_ROW_COUNT)
            .filter(|index| index % 10 != 0)
            .count() as i64;

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS total, COUNT(score) AS non_null, SUM(score) AS sum_score, AVG(score) AS avg_score, MIN(score) AS min_score, MAX(score) AS max_score FROM exec_parallel_aggregation_nulls",
                vec![],
            )
            .expect("parallel ungrouped aggregate should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![
                Value::Int64(PARALLEL_ROW_COUNT as i64),
                Value::Int64(non_null_rows),
                Value::Int64(non_null_rows),
                Value::Float64(1.0),
                Value::Int64(1),
                Value::Int64(1),
            ]]
        );
        assert!(metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_having_order_limit_offset_for_parallel_aggregation() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_aggregation_having_order");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_having";
        let session = cassie.create_session("tester", None);
        create_registered_collection(
            &cassie,
            collection,
            &[("category", DataType::Text), ("score", DataType::Int)],
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = match index % 4 {
                    0 => "a",
                    1 => "b",
                    2 => "c",
                    _ => "d",
                };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "category": category,
                        "score": 1,
                    }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM exec_parallel_aggregation_having GROUP BY category HAVING COUNT(*) >= 256 ORDER BY category LIMIT 2 OFFSET 1",
                vec![],
            )
            .expect("parallel aggregate query should preserve downstream clauses");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("b".to_string()), Value::Int64(256)],
                vec![Value::String("c".to_string()), Value::Int64(256)],
            ]
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_aggregation_for_distinct() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_aggregation_distinct_fallback");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_distinct";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("category", DataType::Text)]);
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = if index % 2 == 0 { "even" } else { "odd" };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({ "category": category }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT category, COUNT(*) AS total FROM exec_parallel_aggregation_distinct GROUP BY category ORDER BY category",
                vec![],
            )
            .expect("distinct aggregate fallback should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(metrics["parallel_aggregation"]["fallback_aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("distinct")
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_aggregation_when_worker_limit_is_one() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_aggregation_single_worker");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_aggregation_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_aggregation_single_worker (score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_parallel_aggregation_single_worker (score) VALUES (7)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS total, SUM(score) AS sum_score FROM exec_parallel_aggregation_single_worker",
                vec![],
            )
            .expect("single-worker aggregate should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(1), Value::Int64(7)]]);
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(metrics["parallel_aggregation"]["fallback_aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("worker-limit-one")
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_aggregation_for_user_defined_function() {
    // Arrange
    with_fallback();
    let path = data_dir("parallel_aggregation_udf_fallback");
    let mut config = CassieRuntimeConfig::from_env();
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_udf";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("score", DataType::Int)]);
        cassie
            .execute_sql(
                &session,
                "CREATE FUNCTION agg_identity(x INT) RETURNS INT AS \"x\"",
                vec![],
            )
            .unwrap();
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT)
                .map(|index| (format!("doc-{index:04}"), serde_json::json!({ "score": 1 }))),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT SUM(agg_identity(score)) AS total FROM exec_parallel_aggregation_udf",
                vec![],
            )
            .expect("udf aggregate fallback should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Int64(PARALLEL_ROW_COUNT as i64)]]
        );
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(
            metrics["parallel_aggregation"]["fallback_aggregations"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("unsupported-expression")
        );
    });

    let _ = std::fs::remove_dir_all(path);
}
