#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_execute_sql_with_non_recursive_cte() {
    // Arrange
    with_fallback();
    let path = data_dir("cte_non_recursive");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "integration_cte";

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
                serde_json::json!({"title": "alpha", "body": "hello"}),
            )

            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("d2".to_string()),
                serde_json::json!({"title": "beta", "body": "world"}),
            )

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "WITH docs_cte AS (SELECT title FROM integration_cte WHERE title = 'alpha') SELECT title FROM docs_cte",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], cassie::types::Value::String("alpha".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_sql_with_recursive_cte() {
    // Arrange
    with_fallback();
    let path = data_dir("cte_recursive");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "integration_recursive_cte";

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
            .put_document(collection, Some("d1".to_string()), serde_json::json!({"n": 1}))

            .unwrap();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "WITH RECURSIVE counter(n) AS (SELECT n FROM integration_recursive_cte WHERE n = 1 UNION ALL SELECT CAST(n + 1 AS INT) FROM counter WHERE n < 2) SELECT n FROM counter ORDER BY n",
            vec![],
            )

.unwrap();

        let rows = result
            .rows
            .into_iter()
            .map(|row| match row.first() {
                Some(Value::Int64(value)) => *value,
                _ => panic!("expected integer value"),
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(rows, vec![1, 2]);
        let _ = std::fs::remove_dir_all(path);
    });
}
