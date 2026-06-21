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
fn should_explain_vector_prefilter_for_indexed_equality_filter() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_vector_prefilter_indexed");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_explain_vector_prefilter_indexed (status TEXT, embedding VECTOR(2), title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_vector_prefilter_status_idx ON sql_explain_vector_prefilter_indexed USING btree (status)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_explain_vector_prefilter_indexed",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "embedding": [1.0, 0.0], "title": "alpha"}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_explain_vector_prefilter_indexed WHERE status = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=index=sql_explain_vector_prefilter_status_idx"));
        assert!(plan.contains("index_aware=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_vector_metadata_prefilter_for_supported_predicates() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_prefilter_supported_predicates");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_prefilter_supported_predicates (status TEXT, rating INT, category TEXT, archived_at TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "rating": 5, "category": "alpha", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d2".to_string()),
                serde_json::json!({"status": "approved", "rating": 5, "category": "alpha", "archived_at": null, "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d3".to_string()),
                serde_json::json!({"status": "approved", "rating": 3, "category": "alpha", "archived_at": null, "embedding": [2.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_supported_predicates",
                Some("d4".to_string()),
                serde_json::json!({"status": "pending", "rating": 5, "category": "alpha", "archived_at": null, "embedding": [1.0, 0.0]}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_supported_predicates WHERE (status = 'approved') AND (rating BETWEEN 4 AND 6) AND (category IN ('alpha', 'beta')) AND archived_at IS NULL ORDER BY distance ASC LIMIT 2",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));
        assert_eq!(result.rows[1][0], Value::String("d2".to_string()));
        assert!(matches!(result.rows[0][1], Value::Float64(value) if value == 0.0));
        assert!(matches!(result.rows[1][1], Value::Float64(value) if value == 0.0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fall_back_for_unsupported_vector_metadata_predicate_without_changing_results() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_prefilter_unsupported_predicate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_vector_prefilter_unsupported_predicate (status TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_unsupported_predicate",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "embedding": [1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_vector_prefilter_unsupported_predicate",
                Some("d2".to_string()),
                serde_json::json!({"status": "pending", "embedding": [0.0, 1.0]}),
            )
            .unwrap();

        // Act
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_unsupported_predicate WHERE lower(status) = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM sql_vector_prefilter_unsupported_predicate WHERE lower(status) = 'approved' ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &explain.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=fallback=unsupported metadata predicate"));
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("d1".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_explain_hybrid_prefilter_for_indexed_equality_filter() {
    // Arrange
    with_fallback();
    let path = data_dir("explain_hybrid_prefilter_indexed");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_explain_hybrid_prefilter_indexed (status TEXT, body TEXT, embedding VECTOR(2))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_hybrid_prefilter_status_idx ON sql_explain_hybrid_prefilter_indexed USING btree (status)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                "sql_explain_hybrid_prefilter_indexed",
                Some("d1".to_string()),
                serde_json::json!({"status": "approved", "body": "red", "embedding": [1.0, 0.0]}),
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, hybrid_score(search_score(body, 'red'), vector_score(embedding, '[1,0]')) AS score FROM sql_explain_hybrid_prefilter_indexed WHERE status = 'approved' ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("prefilter=index=sql_explain_hybrid_prefilter_status_idx"));
        assert!(plan.contains("index_aware=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_vector_index_when_embedding_dimensions_mismatch() {
    // Arrange
    with_fallback();
    let path = data_dir("vector_index_embedding_dimension_mismatch");
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
                "CREATE TABLE vector_index_embedding_dimension_mismatch (content TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();

        // Act
        let created = cassie
            .execute_sql(
                &session,
                "CREATE INDEX vector_index_embedding_dimension_mismatch_idx ON vector_index_embedding_dimension_mismatch USING vector (embedding) WITH (source_field = content)",
                vec![],
            );

        // Assert
        assert!(created.is_err());
        assert!(created
            .unwrap_err()
            .to_string()
            .contains("embedding dimension mismatch"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_hnsw_vector_index_options_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_vector_index_options");
    {
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_hnsw_vector_index_options (content TEXT, embedding VECTOR(1536))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_hnsw_vector_index_options_idx ON sql_hnsw_vector_index_options USING vector (embedding) WITH (source_field = content, metric = l2, index_type = hnsw, m = 12, ef_construction = 96, ef_search = 48)",
                vec![],
            )
            .unwrap();
    }

    // Act
    let restarted =
        Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).unwrap();
    restarted.startup().unwrap();
    let index = restarted
        .catalog
        .get_vector_index("sql_hnsw_vector_index_options", "embedding")
        .expect("hnsw vector index should hydrate");

    // Assert
    assert_eq!(index.metadata.index_type, VectorIndexType::Hnsw);
    let hnsw = index.metadata.hnsw.expect("hnsw options");
    assert_eq!(hnsw.m, 12);
    assert_eq!(hnsw.ef_construction, 96);
    assert_eq!(hnsw.ef_search, 48);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rebuild_normalized_vector_sidecars_after_sql_writes() {
    // Arrange
    with_fallback();
    let path = data_dir("normalized_sidecar_sql_rebuild");
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
                "CREATE TABLE normalized_sidecar_sql_rebuild (title TEXT, embedding VECTOR(3))",
                vec![],
            )
            .unwrap();

        let row_id = match &cassie
            .execute_sql(
                &session,
                "INSERT INTO normalized_sidecar_sql_rebuild (title, embedding) VALUES ('alpha', $1) RETURNING _id",
                vec![Value::Vector(Vector::new(vec![3.0, 4.0, 0.0]))],
            )
            .unwrap()
            .rows[0][0]
        {
            Value::String(id) => id.clone(),
            other => panic!("expected string row id, got {other:?}"),
        };
        cassie
            .execute_sql(
                &session,
                "UPDATE normalized_sidecar_sql_rebuild SET embedding = $1 WHERE title = 'alpha'",
                vec![Value::Vector(Vector::new(vec![0.0, 0.0, 5.0]))],
            )
            .unwrap();

        let vector_index = VectorIndexRecord {
            collection: "normalized_sidecar_sql_rebuild".to_string(),
            field: "embedding".to_string(),
            source_field: "title".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::Cosine,
                index_type: VectorIndexType::BruteForce,
                hnsw: None,
            },
        };

        // Act
        cassie.midge.put_vector_index(vector_index.clone()).unwrap();
        let stored = cassie
            .midge
            .get_normalized_vector("normalized_sidecar_sql_rebuild", "embedding", &row_id)
            .unwrap()
            .unwrap();

        clear_normalized_sidecars(&cassie, "normalized_sidecar_sql_rebuild", "embedding");
        assert!(
            cassie
                .midge
                .get_normalized_vector("normalized_sidecar_sql_rebuild", "embedding", &row_id)
                .unwrap()
                .is_none()
        );

        cassie
            .midge
            .rebuild_normalized_vectors_for_index(&vector_index)
            .unwrap();
        let rebuilt = cassie
            .midge
            .get_normalized_vector("normalized_sidecar_sql_rebuild", "embedding", &row_id)
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(stored.collection, "normalized_sidecar_sql_rebuild");
        assert_eq!(stored.field, "embedding");
        assert_eq!(stored.id, row_id);
        assert_eq!(stored.dimensions, 3);
        assert_eq!(stored.metric, DistanceMetric::Cosine);
        assert!(stored.payload_available);
        assert_eq!(stored.normalization_version, 1);
        assert_eq!(stored.values, vec![0.0, 0.0, 1.0]);
        assert_eq!(stored.magnitude, 5.0);
        assert_eq!(rebuilt.values, stored.values);
        assert_eq!(rebuilt.magnitude, stored.magnitude);

        let _ = std::fs::remove_dir_all(path);
    });
}
