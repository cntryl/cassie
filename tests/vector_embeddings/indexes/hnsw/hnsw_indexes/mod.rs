use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::embeddings::{
    DistanceMetric, HnswIndexOptions, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
};
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    DocumentWriteFailurePoint, StorageFamily,
};
use cassie::types::{DataType, FieldSchema, Schema, Value};

use super::support_sql as support;
use support::*;

include!("semantics.rs");

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

    assert_eq!(result.rows()[0][0], Value::String("near".to_string()));
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

include!("restart.rs");

include!("resources.rs");

include!("nulls.rs");

include!("errors.rs");

include!("binary.rs");

fn hnsw_layout_fixture(label: &str) -> (Cassie, String, &'static str) {
    let path = data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
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
