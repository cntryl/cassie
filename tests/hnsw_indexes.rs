use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::embeddings::{
    DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value};

#[path = "support/sql.rs"]
mod support;
use support::*;

fn canonical_hnsw_collection(collection: &str) -> String {
    canonical_relation_name("postgres", "public", collection)
}

fn register_hnsw_collection(cassie: &Cassie, collection: &str) {
    let collection = canonical_hnsw_collection(collection);
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
        .create_collection(&collection, schema.clone())
        .unwrap();
    cassie.register_collection(
        &collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
}

fn put_hnsw_document(cassie: &Cassie, collection: &str, id: &str, embedding: [f64; 3]) {
    let collection = canonical_hnsw_collection(collection);
    cassie
        .midge
        .put_document(
            &collection,
            Some(id.to_string()),
            serde_json::json!({"content": id, "embedding": embedding}),
        )
        .unwrap();
}

fn hnsw_index_record(collection: &str, ef_search: usize) -> VectorIndexRecord {
    VectorIndexRecord {
        collection: canonical_hnsw_collection(collection),
        field: "embedding".to_string(),
        source_field: "content".to_string(),
        metadata: VectorIndexMetadata {
            provider: "manual".to_string(),
            model: "manual".to_string(),
            dimensions: 3,
            metric: DistanceMetric::L2,
            index_type: VectorIndexType::Hnsw,
            hnsw: Some(HnswIndexOptions {
                version: 1,
                m: 2,
                ef_construction: 4,
                ef_search,
            }),
            hnsw_graph: None,
            ivfflat: None,
            ivfflat_training: None,
        },
    }
}

fn put_hnsw_index(cassie: &Cassie, collection: &str, ef_search: usize) -> VectorIndexRecord {
    let record = hnsw_index_record(collection, ef_search);
    cassie.midge.put_vector_index(record.clone()).unwrap();
    cassie.register_vector_index(record.clone());
    record
}

fn stored_hnsw_index(cassie: &Cassie, collection: &str) -> VectorIndexRecord {
    let collection = canonical_hnsw_collection(collection);
    cassie
        .midge
        .get_vector_index(&collection, "embedding")
        .unwrap()
        .expect("hnsw vector index should persist")
}

fn clear_stored_hnsw_graph(cassie: &Cassie, collection: &str) {
    mutate_stored_hnsw_index(cassie, collection, |record| {
        record.metadata.hnsw_graph = None;
    });
}

fn mutate_stored_hnsw_index(
    cassie: &Cassie,
    collection: &str,
    mut mutate: impl FnMut(&mut VectorIndexRecord),
) {
    let collection = canonical_hnsw_collection(collection);
    let mut record = cassie
        .midge
        .get_vector_index(&collection, "embedding")
        .unwrap()
        .expect("stored vector index metadata should exist");
    mutate(&mut record);
    cassie
        .midge
        .put_vector_index_state(
            &collection,
            "embedding",
            cassie::embeddings::VectorIndexState {
                built_generation: 0,
                hnsw_graph: record.metadata.hnsw_graph,
                ivfflat_training: record.metadata.ivfflat_training,
            },
        )
        .unwrap();
}

fn assert_hnsw_fallback_query(
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
        after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
            - before["vector"]["hnsw_fallbacks"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        after["vector"]["last_fallback_reason"].as_str(),
        Some(expected_reason)
    );
}

#[test]
fn should_hydrate_persisted_hnsw_graph_state_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_graph_restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let collection = "hnsw_graph_restart";
        register_hnsw_collection(&cassie, collection);
        put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
        put_hnsw_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
        put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
        put_hnsw_index(&cassie, collection, 2);

        let stored = stored_hnsw_index(&cassie, collection);
        let graph = stored.metadata.hnsw_graph.expect("hnsw graph state");
        assert_eq!(graph.row_count, 3);
        assert_eq!(graph.dimensions, 3);
        assert_eq!(graph.metric, DistanceMetric::L2);
        assert!(graph.entry_point.is_some());
        assert_eq!(graph.nodes.len(), 3);
    }

    // Act
    let restarted = Cassie::new_with_data_dir(&path).unwrap();
    restarted.startup().unwrap();
    let stored = stored_hnsw_index(&restarted, "hnsw_graph_restart");

    // Assert
    let graph = stored
        .metadata
        .hnsw_graph
        .expect("hydrated hnsw graph state");
    assert_eq!(graph.row_count, 3);
    assert_eq!(graph.nodes.len(), 3);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_store_hnsw_graph_state_in_the_data_family() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_graph_data_family");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let collection = "hnsw_graph_data_family";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let raw_metadata = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Schema, b"")
        .expect("schema scan")
        .into_iter()
        .find_map(|(_key, value)| serde_json::from_slice::<VectorIndexRecord>(&value).ok())
        .expect("vector index metadata");
    let state = cassie
        .midge
        .get_vector_index_state(&canonical_hnsw_collection(collection), "embedding")
        .expect("read state")
        .expect("persisted state");

    // Assert
    assert!(raw_metadata.metadata.hnsw_graph.is_none());
    assert!(state.hnsw_graph.is_some());
    assert!(state.ivfflat_training.is_none());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_refresh_hnsw_graph_after_document_mutations() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_graph_refresh");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_graph_refresh";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    assert_eq!(
        stored_hnsw_index(&cassie, collection)
            .metadata
            .hnsw_graph
            .unwrap()
            .row_count,
        2
    );

    // Act
    put_hnsw_document(&cassie, collection, "new-nearest", [0.9, 0.0, 0.0]);
    let after_insert = stored_hnsw_index(&cassie, collection);
    cassie
        .midge
        .delete_document(&canonical_hnsw_collection(collection), "far")
        .expect("delete document");
    let after_delete = stored_hnsw_index(&cassie, collection);

    // Assert
    assert_eq!(after_insert.metadata.hnsw_graph.unwrap().row_count, 3);
    assert_eq!(after_delete.metadata.hnsw_graph.unwrap().row_count, 2);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_hnsw_reads_safe_during_concurrent_mutation() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_concurrent_mutation");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_concurrent_mutation";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "middle", [0.5, 0.5, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
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
                        "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_concurrent_mutation ORDER BY distance ASC LIMIT 1",
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
            put_hnsw_document(
                cassie.as_ref(),
                "hnsw_concurrent_mutation",
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
    assert_eq!(
        stored_hnsw_index(cassie.as_ref(), collection)
            .metadata
            .hnsw_graph
            .unwrap()
            .row_count,
        4
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_hnsw_graph_for_sql_vector_top_k_with_bounded_candidates() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_sql_topk");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_sql_topk";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "middle", [0.6, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "orthogonal", [0.0, 1.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_sql_topk ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows[0][0], Value::String("near".to_string()));
    assert_eq!(
        after["vector"]["hnsw_executions"].as_u64().unwrap()
            - before["vector"]["hnsw_executions"].as_u64().unwrap(),
        1
    );
    assert!(
        after["vector"]["candidate_count_total"].as_u64().unwrap()
            - before["vector"]["candidate_count_total"].as_u64().unwrap()
            < 4
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_hnsw_sql_top_k_query_dimension_mismatch() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_sql_topk_dimension_mismatch");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_sql_topk_dimension_mismatch";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    let session = cassie.create_session("tester", None);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0]') AS distance FROM hnsw_sql_topk_dimension_mismatch ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .expect_err("dimension mismatch should fail");

    // Assert
    let error = error.to_string();
    assert!(error.contains("vector_distance query for field 'embedding' on collection"));
    assert!(error.contains("expects 3 dimensions but received 2"));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_deterministically_when_hnsw_graph_is_missing() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_missing_graph_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_missing_graph_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    clear_stored_hnsw_graph(&cassie, collection);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_missing_graph_fallback ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows[0][0], Value::String("near".to_string()));
    assert_eq!(
        after["vector"]["hnsw_fallbacks"].as_u64().unwrap()
            - before["vector"]["hnsw_fallbacks"].as_u64().unwrap(),
        1
    );
    assert_eq!(
        after["vector"]["last_fallback_reason"].as_str(),
        Some("missing-graph")
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_same_row_count_hnsw_graph_fingerprint_is_stale() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_stale_fingerprint_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_stale_fingerprint_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.source_fingerprint ^= 1;
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "stale-source-fingerprint";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_hnsw_graph_neighbor_reference_is_corrupt() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_corrupt_neighbor_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_corrupt_neighbor_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.nodes[0].layers[0].push("missing-neighbor".to_string());
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "unknown-neighbor-id";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_hnsw_graph_entry_point_is_invalid() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_invalid_entry_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_invalid_entry_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.entry_point = Some("missing-entry".to_string());
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "missing-entry-point";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fall_back_when_hnsw_graph_max_layer_is_invalid() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_invalid_max_layer_fallback");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_invalid_max_layer_fallback";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);
    mutate_stored_hnsw_index(&cassie, collection, |record| {
        let graph = record.metadata.hnsw_graph.as_mut().expect("hnsw graph");
        graph.max_layer = graph.max_layer.saturating_add(1);
    });
    let before = cassie.metrics();

    // Act
    let expected_reason = "inconsistent-max-layer";

    // Assert
    assert_hnsw_fallback_query(&cassie, collection, expected_reason, &before);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_store_hnsw_nodes_as_point_readable_binary_records() {
    // Arrange
    let (cassie, path, collection) = hnsw_layout_fixture("hnsw_point_nodes");

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let (_, entries, _, node_count, monolithic_graph_count) =
        inspect_hnsw_layout(&cassie, collection);

    // Assert
    assert_eq!(node_count, 2);
    assert_eq!(monolithic_graph_count, 0);
    assert!(entries.iter().all(|(_, raw)| raw.first() != Some(&0x7b)));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_store_hnsw_manifest_as_binary_record() {
    // Arrange
    let (cassie, path, collection) = hnsw_layout_fixture("hnsw_binary_manifest");

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let (_, _, state_value, _, _) = inspect_hnsw_layout(&cassie, collection);

    // Assert
    assert_eq!(state_value.first(), Some(&3));
    assert_ne!(state_value.first(), Some(&0x7b));

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_omit_names_from_hnsw_hot_keys() {
    // Arrange
    let (cassie, path, collection) = hnsw_layout_fixture("hnsw_numeric_keys");

    // Act
    put_hnsw_index(&cassie, collection, 2);
    let (prefix, _, _, _, _) = inspect_hnsw_layout(&cassie, collection);

    // Assert
    assert!(!prefix
        .windows(collection.len())
        .any(|window| window == collection.as_bytes()));
    assert!(!prefix
        .windows("embedding".len())
        .any(|window| window == b"embedding"));

    let _ = std::fs::remove_dir_all(path);
}

fn hnsw_layout_fixture(label: &str) -> (Cassie, String, &'static str) {
    with_fallback();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_layout_records";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    (cassie, path, collection)
}

type HnswLayoutInspection = (Vec<u8>, Vec<(Vec<u8>, Vec<u8>)>, Vec<u8>, usize, usize);

fn inspect_hnsw_layout(cassie: &Cassie, collection: &str) -> HnswLayoutInspection {
    let prefix = cassie
        .midge
        .hnsw_node_prefix_for_diagnostics(collection, "embedding")
        .unwrap();
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .unwrap();
    let state_key = cassie
        .midge
        .vector_state_key_for_diagnostics(collection, "embedding")
        .unwrap();
    let state_value = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &state_key)
        .unwrap()
        .into_iter()
        .find_map(|(key, value)| (key == state_key).then_some(value))
        .expect("vector state manifest");
    let node_count = entries
        .iter()
        .filter(|(_, raw)| raw.first() == Some(&2))
        .count();
    let monolithic_graph_count = entries
        .iter()
        .filter(|(_, raw)| {
            serde_json::from_slice::<cassie::embeddings::HnswGraphState>(raw).is_ok()
        })
        .count();
    (
        prefix,
        entries,
        state_value,
        node_count,
        monolithic_graph_count,
    )
}

#[test]
fn should_scale_hnsw_query_reads_with_reachable_nodes_not_corpus_size() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_point_read_scaling");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_point_read_scaling";
    register_hnsw_collection(&cassie, collection);
    for index in 0..32 {
        let value = if index == 0 { 1.0 } else { -1.0 };
        put_hnsw_document(
            &cassie,
            collection,
            &format!("doc-{index}"),
            [value, 0.0, 0.0],
        );
    }
    put_hnsw_index(&cassie, collection, 2);
    let before = cassie.metrics();
    let session = cassie.create_session("tester", None);

    // Act
    cassie
        .execute_sql(
            &session,
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM hnsw_point_read_scaling ORDER BY distance ASC LIMIT 1",
            vec![],
        )
        .unwrap();
    let after = cassie.metrics();

    // Assert
    let reads = after["storage"]["data"]["reads"].as_u64().unwrap()
        - before["storage"]["data"]["reads"].as_u64().unwrap();
    assert!(reads < 32, "expected bounded point reads, observed {reads}");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_remove_hnsw_point_nodes_when_index_is_dropped() {
    // Arrange
    with_fallback();
    let path = data_dir("hnsw_point_nodes_drop_cleanup");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let collection = "hnsw_point_nodes_drop_cleanup";
    register_hnsw_collection(&cassie, collection);
    put_hnsw_document(&cassie, collection, "near", [1.0, 0.0, 0.0]);
    put_hnsw_document(&cassie, collection, "far", [-1.0, 0.0, 0.0]);
    put_hnsw_index(&cassie, collection, 2);

    // Act
    cassie
        .midge
        .delete_vector_index(&canonical_hnsw_collection(collection), "embedding")
        .unwrap();
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();

    // Assert
    assert!(!entries.iter().any(|(_, raw)| {
        serde_json::from_slice::<cassie::embeddings::HnswGraphNode>(raw).is_ok()
    }));

    let _ = std::fs::remove_dir_all(path);
}
