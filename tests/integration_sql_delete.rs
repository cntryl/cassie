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
fn should_execute_delete_where_returning_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_where_returning");
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
                "CREATE TABLE delete_where_returning (title TEXT, status TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_where_returning (title, status) VALUES ('alpha', 'old')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_where_returning (title, status) VALUES ('beta', 'old')",
                vec![],
            )
            .unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_where_returning WHERE title = 'alpha' RETURNING _id, title",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM delete_where_returning ORDER BY title ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(deleted.command, "DELETE 1");
        assert_eq!(deleted.rows.len(), 1);
        assert!(matches!(&deleted.rows[0][0], Value::String(id) if !id.is_empty()));
        assert_eq!(deleted.rows[0][1], Value::String("alpha".to_string()));
        assert_eq!(selected.rows, vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_delete_returning_scalar_function() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_returning_function");
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
                "CREATE TABLE delete_returning_function (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_returning_function (title) VALUES ('ALPHA')",
                vec![],
            )
            .unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_returning_function RETURNING lower(title) AS normalized",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(deleted.columns[0].name, "normalized");
        assert_eq!(deleted.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_zero_rows_for_delete_without_matches() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_no_match");
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
                "CREATE TABLE delete_no_match (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_no_match (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let deleted = cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_no_match WHERE title = 'missing' RETURNING title",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(&session, "SELECT title FROM delete_no_match", vec![])
            .unwrap();

        // Assert
        assert_eq!(deleted.command, "DELETE 0");
        assert!(deleted.rows.is_empty());
        assert_eq!(selected.rows.len(), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_legacy_fallback_key_for_sql_delete() {
    // Arrange
    with_fallback();
    let path = data_dir("delete_legacy_cleanup");
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
                "CREATE TABLE delete_legacy_cleanup (title TEXT)",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "delete_legacy_cleanup");
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO delete_legacy_cleanup (title) VALUES ('alpha') RETURNING _id",
                vec![],
            )
            .unwrap();
        let row_id = match &inserted.rows[0][0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected row id"),
        };
        put_legacy_document(
            &cassie,
            &collection,
            &row_id,
            &serde_json::json!({"title": "stale"}),
        );

        // Act
        cassie
            .execute_sql(
                &session,
                "DELETE FROM delete_legacy_cleanup WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let deleted = cassie.midge.get_document(&collection, &row_id).unwrap();

        // Assert
        assert!(deleted.is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}
