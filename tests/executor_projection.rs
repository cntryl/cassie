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
fn should_execute_simple_filtered_query() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let path = data_dir("smoke");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "exec_smoke";

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
            .put_document(collection, None, serde_json::json!({"title": "alpha"}))
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM exec_smoke WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.columns[0].name, "title");
        assert_eq!(result.rows.len(), 1);
        match &result.rows[0][0] {
            Value::String(value) => assert_eq!(value, "alpha"),
            _ => panic!("expected string in first column"),
        }

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_query_across_multiple_batches_without_truncation() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_multi_batch";

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

        let documents = (0..1029)
            .map(|index| {
                let id = format!("d{index:04}");
                let title = format!("doc-{index:04}");
                (Some(id), serde_json::json!({ "title": title }))
            })
            .collect::<Vec<_>>();
        cassie.midge.put_documents(collection, documents).unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_multi_batch ORDER BY title ASC LIMIT 5 OFFSET 1024",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 5);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "d1024".to_string(),
                "d1025".to_string(),
                "d1026".to_string(),
                "d1027".to_string(),
                "d1028".to_string(),
            ]
        );
    });
}

#[test]
fn should_preserve_filtered_projection_across_multiple_batches() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_multi_batch_filter";

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
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            );

        let documents = (0..1030)
            .map(|index| {
                let id = format!("d{index:04}");
                let title = format!("doc-{index:04}");
                let status = if index % 2 == 0 { "keep" } else { "drop" };
                (
                    Some(id),
                    serde_json::json!({ "title": title, "status": status }),
                )
            })
            .collect::<Vec<_>>();
        cassie.midge.put_documents(collection, documents).unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM exec_multi_batch_filter WHERE status = 'keep' ORDER BY title ASC LIMIT 5 OFFSET 510",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 5);
        let ids = result
            .rows
            .into_iter()
            .map(|row| match &row[0] {
                Value::String(value) => value.clone(),
                _ => panic!("expected id string"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            vec![
                "d1020".to_string(),
                "d1022".to_string(),
                "d1024".to_string(),
                "d1026".to_string(),
                "d1028".to_string(),
            ]
        );
    });
}

#[test]
fn should_project_missing_columns_as_null() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        with_fallback();
        let cassie = Cassie::new().unwrap();
        let collection = "exec_missing_projection_column";

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
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM exec_missing_projection_column",
                vec![],
            )
            .expect("query should execute");

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].len(), 2);
        assert_eq!(result.rows[0][0], Value::String("alpha".to_string()));
        assert_eq!(result.rows[0][1], Value::Null);
    });
}

#[test]
fn should_project_complex_values_through_filtered_ordered_scan() {
    // Arrange
    with_fallback();
    let path = data_dir("zero_copy_projected_complex_values");
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
                "CREATE TABLE zero_copy_projected_complex_values (title TEXT, score INT, payload JSON, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "zero_copy_projected_complex_values",
                Some("doc-1".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "score": 2,
                    "payload": {"nested": ["a", "b"]},
                    "embedding": [1.0, 2.0],
                }),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "zero_copy_projected_complex_values",
                Some("doc-2".to_string()),
                serde_json::json!({
                    "title": "alpha",
                    "score": 1,
                    "embedding": [3.0, 4.0],
                }),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT payload, embedding FROM zero_copy_projected_complex_values WHERE title = 'alpha' ORDER BY score ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::Null);
        assert_eq!(
            result.rows[0][1],
            Value::Vector(cassie::types::Vector::new(vec![3.0, 4.0]))
        );
        assert_eq!(
            result.rows[1][0],
            Value::Json(serde_json::json!({"nested": ["a", "b"]}))
        );
        assert_eq!(
            result.rows[1][1],
            Value::Vector(cassie::types::Vector::new(vec![1.0, 2.0]))
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
