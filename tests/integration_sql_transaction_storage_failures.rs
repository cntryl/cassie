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
fn should_not_persist_row_when_row_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_row_failpoint");
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
                "CREATE TABLE write_row_failpoint (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::Row));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_row_failpoint (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(&session, "SELECT id FROM write_row_failpoint", vec![])
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_row_failpoint (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT title FROM write_row_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(
            after_retry.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_scalar_index_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_scalar_index_failpoint");
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
                "CREATE TABLE write_scalar_index_failpoint (id INT PRIMARY KEY, email TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_scalar_index_failpoint_email_idx ON write_scalar_index_failpoint USING btree (email)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::ScalarIndex));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_scalar_index_failpoint (id, email) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT email FROM write_scalar_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_scalar_index_failpoint (id, email) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT email FROM write_scalar_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(after_retry.rows, vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_time_series_index_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_time_series_index_failpoint");
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
                "CREATE TABLE write_time_series_index_failpoint (id INT PRIMARY KEY, tenant TEXT, event_at TIMESTAMP)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_time_series_index_failpoint_ts_idx ON write_time_series_index_failpoint USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::TimeSeriesIndex));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_time_series_index_failpoint (id, tenant, event_at) VALUES (1, 'acme', '2026-01-01T00:00:00Z')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT id FROM write_time_series_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_time_series_index_failpoint (id, tenant, event_at) VALUES (1, 'acme', '2026-01-01T00:00:00Z')",
                vec![],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT tenant FROM write_time_series_index_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(after_retry.rows, vec![vec![Value::String("acme".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_graph_adjacency_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_graph_adjacency_failpoint");
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
                "CREATE GRAPH social_graph_failpoint (NODES (label TEXT), EDGES (source TEXT))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO social_graph_failpoint_nodes (node_type, node_id, label) VALUES ('person', 'alice', 'Alice'), ('person', 'bob', 'Bob')",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::GraphAdjacency));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO social_graph_failpoint_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct')",
                vec![],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM social_graph_failpoint_edges WHERE edge_id = 'e1'",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(failed.to_string().contains("injected test failure"));

        cassie
            .execute_sql(
                &session,
                "INSERT INTO social_graph_failpoint_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight, source) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1, 'direct')",
                vec![],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT source FROM social_graph_failpoint_edges WHERE edge_id = 'e1'",
                vec![],
            )
            .unwrap();
        assert_eq!(after_retry.rows, vec![vec![Value::String("direct".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_not_persist_document_when_normalized_vector_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_normalized_vector_failpoint");
    {
        let mut config = CassieRuntimeConfig::from_env().unwrap();
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "rollback-test".to_string(),
            dimensions: 3,
        });
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE write_normalized_vector_failpoint (id INT PRIMARY KEY, content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_normalized_vector_failpoint_idx ON write_normalized_vector_failpoint USING vector (embedding) WITH (source_field = content, index_type = hnsw)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::NormalizedVector));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_normalized_vector_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT id FROM write_normalized_vector_failpoint",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(
            failed.to_string().contains("injected test failure"),
            "{failed}"
        );

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_normalized_vector_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT content FROM write_normalized_vector_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(
            after_retry.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

#[test]
fn should_not_persist_document_when_vector_state_family_failpoint_is_triggered() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    with_fallback();
    let path = data_dir("write_vector_state_failpoint");
    {
        let mut config = CassieRuntimeConfig::from_env().unwrap();
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "rollback-test".to_string(),
            dimensions: 3,
        });
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE write_vector_state_failpoint (id INT PRIMARY KEY, content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX write_vector_state_failpoint_idx ON write_vector_state_failpoint USING vector (embedding) WITH (source_field = content, index_type = hnsw)",
                vec![],
            )
            .unwrap();

        // Act
        set_document_write_failure_point(Some(DocumentWriteFailurePoint::VectorState));
        let failed = cassie
            .execute_sql(
                &session,
                "INSERT INTO write_vector_state_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap_err();
        set_document_write_failure_point(None);

        // Assert
        let before_retry = cassie
            .execute_sql(
                &session,
                "SELECT id FROM write_vector_state_failpoint",
                vec![],
            )
            .unwrap();
        assert!(before_retry.rows.is_empty());
        assert!(
            failed.to_string().contains("injected test failure"),
            "{failed}"
        );

        cassie
            .execute_sql(
                &session,
                "INSERT INTO write_vector_state_failpoint (id, content, embedding) VALUES (1, 'alpha', $1)",
                vec![Value::Vector(Vector::new(vec![0.1, 0.2, 0.3]))],
            )
            .unwrap();
        let after_retry = cassie
            .execute_sql(
                &session,
                "SELECT content FROM write_vector_state_failpoint WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_eq!(
            after_retry.rows,
            vec![vec![Value::String("alpha".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    }
}
