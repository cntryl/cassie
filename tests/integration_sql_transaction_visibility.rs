#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig, OpenAiRuntimeConfig,
};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    DocumentWriteFailurePoint,
};
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;
#[test]
fn should_hide_transaction_writes_from_other_sessions_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_uncommitted_visibility");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        cassie
            .execute_sql(
                &writer,
                "CREATE TABLE transaction_uncommitted_visibility (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&writer, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &writer,
                "INSERT INTO transaction_uncommitted_visibility (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &reader,
                "SELECT title FROM transaction_uncommitted_visibility",
                vec![],
            )
            .unwrap();

        // Assert
        assert!(selected.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_own_transaction_writes_before_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_read_your_writes");
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
                "CREATE TABLE transaction_read_your_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_read_your_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM transaction_read_your_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_persist_transaction_writes_after_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_commit_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let writer = cassie.create_session("writer", None);
        let reader = cassie.create_session("reader", None);
        cassie
            .execute_sql(
                &writer,
                "CREATE TABLE transaction_commit_writes (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&writer, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &writer,
                "INSERT INTO transaction_commit_writes (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        cassie.execute_sql(&writer, "COMMIT", vec![]).unwrap();
        let selected = cassie
            .execute_sql(
                &reader,
                "SELECT title FROM transaction_commit_writes",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_transaction_insert_out_of_storage_until_commit() {
    // Arrange
    with_fallback();
    let path = data_dir("transaction_storage_routing");
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
                "CREATE TABLE transaction_storage_routing (title TEXT)",
                vec![],
            )
            .unwrap();
        let collection = cassie
            .catalog
            .get_schema("transaction_storage_routing")
            .expect("catalog collection")
            .collection;
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_storage_routing (title) VALUES ('alpha') RETURNING _id",
                vec![],
            )
            .unwrap();
        let row_id = match &inserted.rows[0][0] {
            Value::String(value) => value.clone(),
            _ => panic!("expected row id"),
        };
        let before_commit = cassie.midge.get_document(&collection, &row_id).unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let after_commit = cassie.midge.get_document(&collection, &row_id).unwrap();

        // Assert
        assert!(before_commit.is_none());
        assert_eq!(
            after_commit.unwrap().payload["title"],
            serde_json::Value::String("alpha".to_string())
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
