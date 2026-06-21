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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_scoring_fulltext (id TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for index in 0..1100 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO exec_parallel_scoring_fulltext (id, body) VALUES ($1, 'alpha beta')",
                    vec![Value::String(format!("doc-{index:04}"))],
                )
                .unwrap();
        }

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
        for index in 0..1100 {
            cassie
                .midge
                .put_document(
                    collection,
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({
                        "id": format!("doc-{index:04}"),
                        "title": format!("title-{index:04}"),
                    }),
                )
                .unwrap();
        }
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT * FROM exec_parallel_scan_merge", vec![])
            .expect("parallel scan query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 1100);
        assert!(metrics["parallel_scans"]["scans"].as_u64().unwrap_or(0) > 0);
        assert!(metrics["parallel_scans"]["workers"].as_u64().unwrap_or(0) >= 2);
        assert_eq!(metrics["parallel_scans"]["rows"].as_u64(), Some(1100));
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
        for index in 0..1100 {
            let category = if index % 2 == 0 { "even" } else { "odd" };
            cassie
                .midge
                .put_document(
                    collection,
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({
                        "category": category,
                        "score": 1,
                    }),
                )
                .unwrap();
        }
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
                    Value::Int64(550),
                    Value::Int64(550),
                ],
                vec![
                    Value::String("odd".to_string()),
                    Value::Int64(550),
                    Value::Int64(550),
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
        assert_eq!(metrics["parallel_aggregation"]["rows"].as_u64(), Some(1100));
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_aggregation_nulls (score INT)",
                vec![],
            )
            .unwrap();
        for index in 0..1100 {
            if index % 10 == 0 {
                cassie
                    .execute_sql(
                        &session,
                        "INSERT INTO exec_parallel_aggregation_nulls (score) VALUES (NULL)",
                        vec![],
                    )
                    .unwrap();
            } else {
                cassie
                    .execute_sql(
                        &session,
                        "INSERT INTO exec_parallel_aggregation_nulls (score) VALUES (1)",
                        vec![],
                    )
                    .unwrap();
            }
        }

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
                Value::Int64(1100),
                Value::Int64(990),
                Value::Int64(990),
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_aggregation_having (category TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for index in 0..1100 {
            let category = match index % 4 {
                0 => "a",
                1 => "b",
                2 => "c",
                _ => "d",
            };
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO exec_parallel_aggregation_having (category, score) VALUES ($1, 1)",
                    vec![Value::String(category.to_string())],
                )
                .unwrap();
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM exec_parallel_aggregation_having GROUP BY category HAVING COUNT(*) >= 275 ORDER BY category LIMIT 2 OFFSET 1",
                vec![],
            )
            .expect("parallel aggregate query should preserve downstream clauses");

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("b".to_string()), Value::Int64(275)],
                vec![Value::String("c".to_string()), Value::Int64(275)],
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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_aggregation_distinct (category TEXT)",
                vec![],
            )
            .unwrap();
        for index in 0..1100 {
            let category = if index % 2 == 0 { "even" } else { "odd" };
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO exec_parallel_aggregation_distinct (category) VALUES ($1)",
                    vec![Value::String(category.to_string())],
                )
                .unwrap();
        }

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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_aggregation_udf (score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE FUNCTION agg_identity(x INT) RETURNS INT AS \"x\"",
                vec![],
            )
            .unwrap();
        for _ in 0..1100 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO exec_parallel_aggregation_udf (score) VALUES (1)",
                    vec![],
                )
                .unwrap();
        }

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
        assert_eq!(result.rows, vec![vec![Value::Int64(1100)]]);
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
