#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::IndexKind;
use cassie::embeddings::{
    DistanceMetric, IvfFlatIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use cassie::sql::ast::QueryStatement;
use cassie::types::{DataType, FieldSchema, Schema, Value};

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_parse_ivfflat_vector_index_options() {
    // Arrange
    let sql = "CREATE INDEX idx_docs_embedding_ivf ON docs USING vector (embedding) WITH (source_field = content, index_type = ivfflat, lists = 16, probes = 4, training_sample_size = 128, training_seed = 42)";

    // Act
    let parsed = cassie::sql::parse_statement(sql).unwrap();

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected CREATE INDEX");
    };
    assert_eq!(statement.kind, IndexKind::Vector);
    assert_eq!(
        statement.options.get("index_type"),
        Some(&"ivfflat".to_string())
    );
    assert_eq!(statement.options.get("lists"), Some(&"16".to_string()));
    assert_eq!(statement.options.get("probes"), Some(&"4".to_string()));
}

#[test]
fn should_persist_ivfflat_vector_index_options() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_vector_index_options");
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, openai_runtime_for_vectors()).expect("cassie");
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE ivfflat_docs (content TEXT, embedding VECTOR(1536))",
            vec![],
        )
        .unwrap();

    // Act
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX idx_ivfflat_docs_embedding ON ivfflat_docs USING vector (embedding) WITH (source_field = content, metric = l2, index_type = ivfflat, lists = 8, probes = 3, training_sample_size = 64, training_seed = 99)",
            vec![],
        )
        .unwrap();
    let stored = cassie
        .midge
        .get_vector_index("ivfflat_docs", "embedding")
        .unwrap()
        .expect("ivfflat vector index should persist");

    // Assert
    assert_eq!(stored.metadata.index_type, VectorIndexType::IvfFlat);
    let ivfflat = stored.metadata.ivfflat.expect("ivfflat options");
    assert_eq!(ivfflat.lists, 8);
    assert_eq!(ivfflat.probes, 3);
    assert_eq!(ivfflat.training_sample_size, 64);
    assert_eq!(ivfflat.training_seed, 99);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_trained_ivfflat_candidate_lists_for_top_k() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_candidate_lists");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_candidate_lists";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(3),
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
                Some("near".to_string()),
                serde_json::json!({"content": "near", "embedding": [1.0, 0.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("orthogonal".to_string()),
                serde_json::json!({"content": "orthogonal", "embedding": [0.0, 1.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("far".to_string()),
                serde_json::json!({"content": "far", "embedding": [-1.0, 0.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_vector_index(VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "content".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::IvfFlat,
                    hnsw: None,
                    ivfflat: Some(IvfFlatIndexOptions {
                        version: 1,
                        lists: 2,
                        probes: 1,
                        training_sample_size: 3,
                        training_seed: 7,
                    }),
                    ivfflat_training: None,
                },
            })
            .unwrap();
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let stored = cassie
            .midge
            .get_vector_index(collection, "embedding")
            .unwrap()
            .expect("ivfflat vector index should persist");
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM ivfflat_candidate_lists ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        let training = stored
            .metadata
            .ivfflat_training
            .expect("ivfflat training state");
        assert!(training.trained);
        assert_eq!(training.row_count, 3);
        assert_eq!(training.lists, 2);
        assert_eq!(training.probes, 1);
        assert_eq!(training.assignments.len(), 3);
        assert_eq!(training.list_sizes.iter().sum::<usize>(), 3);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        let vector_count_delta =
            after["vector"]["count"].as_u64().unwrap() - before["vector"]["count"].as_u64().unwrap();
        let candidate_count_delta = after["vector"]["candidate_count_total"].as_u64().unwrap()
            - before["vector"]["candidate_count_total"]
                .as_u64()
                .unwrap();
        assert_eq!(vector_count_delta, 1);
        assert!(candidate_count_delta < 3);
        assert_eq!(
            after["vector"]["ivfflat_executions"].as_u64().unwrap()
                - before["vector"]["ivfflat_executions"].as_u64().unwrap(),
            1
        );
        assert_eq!(after["vector"]["last_index_kind"].as_str(), Some("ivfflat"));
        assert!(
            after["vector"]["ivfflat_exact_reranks_total"]
                .as_u64()
                .unwrap()
                > before["vector"]["ivfflat_exact_reranks_total"]
                    .as_u64()
                    .unwrap()
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_refresh_ivfflat_training_after_document_writes() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_write_refresh");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "ivfflat_write_refresh";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(3),
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
                Some("near".to_string()),
                serde_json::json!({"content": "near", "embedding": [1.0, 0.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_document(
                collection,
                Some("far".to_string()),
                serde_json::json!({"content": "far", "embedding": [-1.0, 0.0, 0.0]}),
            )
            .unwrap();
        cassie
            .midge
            .put_vector_index(VectorIndexRecord {
                collection: collection.to_string(),
                field: "embedding".to_string(),
                source_field: "content".to_string(),
                metadata: VectorIndexMetadata {
                    provider: "manual".to_string(),
                    model: "manual".to_string(),
                    dimensions: 3,
                    metric: DistanceMetric::L2,
                    index_type: VectorIndexType::IvfFlat,
                    hnsw: None,
                    ivfflat: Some(IvfFlatIndexOptions {
                        version: 1,
                        lists: 2,
                        probes: 1,
                        training_sample_size: 3,
                        training_seed: 11,
                    }),
                    ivfflat_training: None,
                },
            })
            .unwrap();
        assert_eq!(
            cassie
                .midge
                .get_vector_index(collection, "embedding")
                .unwrap()
                .unwrap()
                .metadata
                .ivfflat_training
                .unwrap()
                .row_count,
            2
        );

        // Act
        cassie
            .midge
            .put_document(
                collection,
                Some("new-nearest".to_string()),
                serde_json::json!({"content": "new-nearest", "embedding": [0.9, 0.0, 0.0]}),
            )
            .unwrap();
        let after_insert = cassie
            .midge
            .get_vector_index(collection, "embedding")
            .unwrap()
            .expect("ivfflat vector index should persist");
        let session = cassie.create_session("tester", None);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[0.9,0,0]') AS distance FROM ivfflat_write_refresh ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .delete_document(collection, "new-nearest")
            .unwrap();
        let after_delete = cassie
            .midge
            .get_vector_index(collection, "embedding")
            .unwrap()
            .expect("ivfflat vector index should persist");

        // Assert
        let inserted_training = after_insert
            .metadata
            .ivfflat_training
            .expect("ivfflat training after insert");
        assert_eq!(inserted_training.row_count, 3);
        assert!(inserted_training.assignments.contains_key("new-nearest"));
        assert_eq!(result.rows[0][0], Value::String("new-nearest".to_string()));
        let deleted_training = after_delete
            .metadata
            .ivfflat_training
            .expect("ivfflat training after delete");
        assert_eq!(deleted_training.row_count, 2);
        assert!(!deleted_training.assignments.contains_key("new-nearest"));

        let _ = std::fs::remove_dir_all(path);
    });
}
