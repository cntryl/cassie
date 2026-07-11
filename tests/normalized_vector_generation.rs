use cassie::app::Cassie;
use cassie::embeddings::{
    DistanceMetric, HnswIndexOptions, NormalizedVectorRecord, VectorIndexMetadata,
    VectorIndexRecord, VectorIndexType,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema};
use cntryl_midge::{TransactionMode, WriteOptions};

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[test]
fn should_reject_normalized_vectors_from_an_older_collection_generation() {
    // Arrange
    with_fallback();
    let path = data_dir("normalized_vector_generation");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let collection = "normalized_vector_generation_docs";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "embedding".to_string(),
            data_type: DataType::Vector(3),
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .expect("create collection");
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
            Some("doc-1".to_string()),
            serde_json::json!({"embedding": [1.0, 0.0, 0.0]}),
        )
        .expect("insert document");
    cassie
        .midge
        .put_vector_index(VectorIndexRecord {
            collection: collection.to_string(),
            field: "embedding".to_string(),
            source_field: "embedding".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::L2,
                index_type: VectorIndexType::Hnsw,
                hnsw: Some(HnswIndexOptions::default()),
                hnsw_graph: None,
                ivfflat: None,
                ivfflat_training: None,
            },
        })
        .expect("create vector index");

    // Act
    rewrite_sidecar_generation(&cassie, collection, 0);

    // Assert
    assert!(cassie
        .midge
        .list_normalized_vectors(collection, "embedding")
        .expect("read sidecars")
        .is_empty());

    let _ = std::fs::remove_dir_all(path);
}

fn rewrite_sidecar_generation(cassie: &Cassie, collection: &str, generation: u64) {
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan data");
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("open data transaction");
    for (key, raw) in entries {
        let Ok(mut record) = serde_json::from_slice::<NormalizedVectorRecord>(&raw) else {
            continue;
        };
        if record.collection == collection && record.field == "embedding" {
            record.built_generation = generation;
            tx.put(
                key,
                serde_json::to_vec(&record).expect("serialize sidecar"),
                None,
            )
            .expect("write stale sidecar");
        }
    }
    tx.commit(WriteOptions::sync())
        .expect("commit stale sidecar");
}
