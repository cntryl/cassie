#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::IndexKind;
use cassie::embeddings::{
    DistanceMetric, IvfFlatIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use cassie::midge::adapter::StorageFamily;
use cassie::sql::ast::QueryStatement;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::*;

fn register_ivfflat_collection(cassie: &Cassie, collection: &str) {
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
}

fn put_ivfflat_document(cassie: &Cassie, collection: &str, id: &str, embedding: [f64; 3]) {
    cassie
        .midge
        .put_document(
            collection,
            Some(id.to_string()),
            serde_json::json!({"content": id, "embedding": embedding}),
        )
        .unwrap();
}

fn put_ivfflat_index(cassie: &Cassie, collection: &str, seed: u64) {
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
                hnsw_graph: None,
                ivfflat: Some(IvfFlatIndexOptions {
                    version: 1,
                    lists: 2,
                    probes: 1,
                    training_sample_size: 3,
                    training_seed: seed,
                }),
                ivfflat_training: None,
            },
        })
        .unwrap();
}

fn stored_ivfflat_index(cassie: &Cassie, collection: &str) -> VectorIndexRecord {
    cassie
        .midge
        .get_vector_index(collection, "embedding")
        .unwrap()
        .expect("ivfflat vector index should persist")
}

fn mutate_stored_ivfflat_index(
    cassie: &Cassie,
    collection: &str,
    mut mutate: impl FnMut(&mut VectorIndexRecord),
) {
    let mut record = cassie
        .midge
        .get_vector_index(collection, "embedding")
        .unwrap()
        .expect("stored vector index metadata should exist");
    mutate(&mut record);
    cassie
        .midge
        .put_vector_index_state(
            collection,
            "embedding",
            cassie::embeddings::VectorIndexState {
                built_generation: 0,
                hnsw_graph: record.metadata.hnsw_graph,
                ivfflat_training: record.metadata.ivfflat_training,
            },
        )
        .unwrap();
}

fn ivfflat_row_count(cassie: &Cassie, collection: &str) -> usize {
    stored_ivfflat_index(cassie, collection)
        .metadata
        .ivfflat_training
        .unwrap()
        .row_count
}

fn assert_candidate_list_training(stored: VectorIndexRecord) {
    let training = stored
        .metadata
        .ivfflat_training
        .expect("ivfflat training state");
    assert!(training.trained);
    assert_ne!(training.source_fingerprint, 0);
    assert_eq!(training.row_count, 3);
    assert_eq!(training.lists, 2);
    assert_eq!(training.probes, 1);
    assert_eq!(training.assignments.len(), 3);
    assert_eq!(training.list_sizes.iter().sum::<usize>(), 3);
}

fn assert_candidate_list_metrics(before: &serde_json::Value, after: &serde_json::Value) {
    let vector_count_delta =
        after["vector"]["count"].as_u64().unwrap() - before["vector"]["count"].as_u64().unwrap();
    let candidate_count_delta = after["vector"]["candidate_count_total"].as_u64().unwrap()
        - before["vector"]["candidate_count_total"].as_u64().unwrap();
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
}

fn assert_ivfflat_fallback_query(
    cassie: &Cassie,
    collection: &str,
    expected_reason: &str,
    before: &serde_json::Value,
) {
    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            &format!(
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {collection} ORDER BY distance ASC LIMIT 1"
            ),
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    assert_eq!(result.rows[0][0], Value::String("near".to_string()));
    assert_eq!(
        after["vector"]["ivfflat_fallbacks"].as_u64().unwrap()
            - before["vector"]["ivfflat_fallbacks"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        after["vector"]["last_fallback_reason"].as_str(),
        Some(expected_reason)
    );
}

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
    let collection = canonical_test_collection(&cassie, "ivfflat_docs");

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
        .get_vector_index(&collection, "embedding")
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
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 7);
        let before = cassie.metrics();
        let session = cassie.create_session("tester", None);

        // Act
        let stored = stored_ivfflat_index(&cassie, collection);
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM ivfflat_candidate_lists ORDER BY distance ASC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_candidate_list_training(stored);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], Value::String("near".to_string()));
        assert_candidate_list_metrics(&before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_read_only_probed_ivfflat_candidates() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_point_reads");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "ivfflat_point_reads";
    register_ivfflat_collection(&cassie, collection);
    for index in 0..32 {
        let embedding = if index < 16 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        put_ivfflat_document(&cassie, collection, &format!("doc-{index}"), embedding);
    }
    put_ivfflat_index(&cassie, collection, 7);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);
    let relation = cassie
        .catalog
        .get_schema(collection)
        .expect("registered collection schema")
        .collection
        .clone();

    // Act
    cassie
        .execute_sql(
            &session,
            &format!("SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM {relation} ORDER BY distance ASC LIMIT 1"),
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
        - before["storage"]["data"]["reads"].as_u64().unwrap();
    assert!(reads < 32, "expected probed-list reads, observed {reads}");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_read_persisted_ivfflat_candidate_ids_without_source_scan() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_persisted_candidate_ids");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "ivfflat_persisted_candidate_ids";
    register_ivfflat_collection(&cassie, collection);
    for index in 0..32 {
        let embedding = if index < 16 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        put_ivfflat_document(&cassie, collection, &format!("doc-{index}"), embedding);
    }
    put_ivfflat_index(&cassie, collection, 31);
    let before = cassie.metrics();

    // Act
    let candidates = cassie
        .midge
        .persisted_vector_candidate_ids(collection, "embedding", &[1.0, 0.0, 0.0], 32)
        .unwrap()
        .expect("persisted ivfflat candidates");
    let after = cassie.metrics();

    // Assert
    assert!(!candidates.is_empty());
    assert!(candidates.len() <= 32);
    let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
        - before["storage"]["data"]["reads"].as_u64().unwrap();
    assert!(
        reads < 32,
        "expected persisted candidate reads, observed {reads}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_store_ivfflat_membership_outside_training_manifest() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_membership_layout");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "ivfflat_membership_layout";
    register_ivfflat_collection(&cassie, collection);
    put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_ivfflat_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);

    // Act
    put_ivfflat_index(&cassie, collection, 17);
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();
    let state = entries
        .iter()
        .filter_map(|(_, raw)| serde_json::from_slice::<serde_json::Value>(raw).ok())
        .find(|value| value.get("ivfflat_training").is_some())
        .expect("persisted ivfflat state");

    // Assert
    assert!(state["ivfflat_training"].get("assignments").is_none());

    let _ = std::fs::remove_dir_all(path);
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
        register_ivfflat_collection(&cassie, collection);
        put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_ivfflat_index(&cassie, collection, 11);
        assert_eq!(ivfflat_row_count(&cassie, collection), 2);

        // Act
        put_ivfflat_document(&cassie, collection, "new-nearest", [0.9, 0.0, 0.0]);
        let after_insert = stored_ivfflat_index(&cassie, collection);
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
        let after_delete = stored_ivfflat_index(&cassie, collection);

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

#[test]
fn should_keep_ivfflat_reads_safe_during_concurrent_mutation() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_concurrent_mutation");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "ivfflat_concurrent_mutation";
    register_ivfflat_collection(&cassie, collection);
    put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_ivfflat_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
    put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_ivfflat_index(&cassie, collection, 41);
    let cassie = std::sync::Arc::new(cassie);

    // Act
    let readers = (0..4)
        .map(|_| {
            let cassie = std::sync::Arc::clone(&cassie);
            std::thread::spawn(move || {
                let session = cassie.create_session("tester", None);
                cassie
                    .execute_sql(
                        &session,
                        "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM ivfflat_concurrent_mutation ORDER BY distance ASC LIMIT 1",
                        vec![],
                    )
                    .unwrap()
                    .rows[0][0]
                    .clone()
            })
        })
        .collect::<Vec<_>>();
    let writer = {
        let cassie = std::sync::Arc::clone(&cassie);
        std::thread::spawn(move || {
            put_ivfflat_document(
                cassie.as_ref(),
                "ivfflat_concurrent_mutation",
                "new-nearest",
                [0.99, 0.0, 0.0],
            );
        })
    };
    writer.join().unwrap();
    let results = readers
        .into_iter()
        .map(|reader| reader.join().unwrap())
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(results.len(), 4);
    assert!(results.into_iter().all(|value| {
        value == Value::String("near".to_string())
            || value == Value::String("new-nearest".to_string())
    }));
    assert_eq!(ivfflat_row_count(cassie.as_ref(), collection), 4);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_ivfflat_training_assignment_coverage_is_missing() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_missing_assignment_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "ivfflat_missing_assignment_fallback";
    register_ivfflat_collection(&cassie, collection);
    put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_ivfflat_index(&cassie, collection, 21);
    mutate_stored_ivfflat_index(&cassie, collection, |record| {
        let training = record
            .metadata
            .ivfflat_training
            .as_mut()
            .expect("ivfflat training");
        training.assignments.remove("far");
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "incomplete-assignments";

    // Assert
    assert_ivfflat_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_ivfflat_training_list_bounds_are_bad() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_bad_list_bounds_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "ivfflat_bad_list_bounds_fallback";
    register_ivfflat_collection(&cassie, collection);
    put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_ivfflat_index(&cassie, collection, 22);
    mutate_stored_ivfflat_index(&cassie, collection, |record| {
        let training = record
            .metadata
            .ivfflat_training
            .as_mut()
            .expect("ivfflat training");
        training
            .assignments
            .insert("near".to_string(), training.lists);
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "empty-probed-lists";

    // Assert
    assert_ivfflat_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_same_row_count_ivfflat_training_fingerprint_is_stale() {
    // Arrange
    with_fallback();
    let path = data_dir("ivfflat_stale_fingerprint_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    let collection = "ivfflat_stale_fingerprint_fallback";
    register_ivfflat_collection(&cassie, collection);
    put_ivfflat_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_ivfflat_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_ivfflat_index(&cassie, collection, 23);
    mutate_stored_ivfflat_index(&cassie, collection, |record| {
        let training = record
            .metadata
            .ivfflat_training
            .as_mut()
            .expect("ivfflat training");
        training.source_fingerprint ^= 1;
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "stale-source-fingerprint";

    // Assert
    assert_ivfflat_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}
