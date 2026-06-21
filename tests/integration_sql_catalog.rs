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
fn should_execute_sql_query_after_catalog_hydration() {
    // Arrange
    with_fallback();
    let path = data_dir("restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "sql_hydration";
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

        cassie.midge.create_collection(collection, schema).unwrap();
        let _ = cassie
            .midge
            .put_document(
                collection,
                None,
                serde_json::json!({"title": "sql", "body": "hybrid path"}),
            )
            .unwrap();

        // Act
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let result = restarted
            .execute_sql(
                &session,
                "SELECT title FROM sql_hydration WHERE title = 'sql'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.columns[0].name, "title");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_namespace_on_create_schema() {
    // Arrange
    with_fallback();
    let path = data_dir("create_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie
            .midge
            .ensure_families_ready()
            .expect("families ready");

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(&session, "CREATE SCHEMA analytics", vec![])
            .unwrap();

        // Assert
        assert_eq!(result.command, "CREATE SCHEMA");
        assert!(cassie.catalog.namespace_exists("analytics"));
        assert!(cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_schema_through_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("rename_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "ALTER SCHEMA reporting RENAME TO reporting_archive",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command, "ALTER SCHEMA");
        assert!(!cassie.catalog.namespace_exists("reporting"));
        assert!(cassie.catalog.namespace_exists("reporting_archive"));
        assert!(!cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting"));
        assert!(cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting_archive"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_drop_schema_through_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("drop_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA reporting", vec![])
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(&session, "DROP SCHEMA reporting", vec![])
            .unwrap();

        // Assert
        assert_eq!(result.command, "DROP SCHEMA");
        assert!(!cassie.catalog.namespace_exists("reporting"));
        assert!(!cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|name| name == "reporting"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_duplicate_create_schema_when_if_not_exists_is_set() {
    // Arrange
    with_fallback();
    let path = data_dir("create_schema_if_not_exists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.midge.create_namespace("analytics").unwrap();

        let initial = cassie.midge.list_namespaces();

        // Act
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(&session, "CREATE SCHEMA IF NOT EXISTS analytics", vec![])
            .unwrap();

        // Assert
        assert_eq!(result.command, "CREATE SCHEMA");
        let namespaced = cassie.midge.list_namespaces();
        assert_eq!(namespaced.len(), initial.len());
        assert!(namespaced.iter().any(|name| name == "analytics"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_column_through_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("rename_column");
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
                "CREATE TABLE rename_column_docs (id TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "rename_column_docs",
                Some("d1".to_string()),
                serde_json::json!({"id": "d1", "title": "alpha"}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "ALTER TABLE rename_column_docs RENAME COLUMN title TO headline",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT id, headline FROM rename_column_docs ORDER BY id",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command, "ALTER TABLE");
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(selected.rows[0][1], Value::String("alpha".to_string()));
        let schema = cassie
            .catalog
            .get_schema("rename_column_docs")
            .expect("schema should exist");
        assert!(schema.fields.iter().any(|field| field.name == "headline"));
        assert!(!schema.fields.iter().any(|field| field.name == "title"));

        let _ = std::fs::remove_dir_all(path);
    });
}
