use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::embeddings::{DistanceMetric, VectorIndexMetadata, VectorIndexRecord};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema};
use cntryl_midge::{TransactionMode, WriteOptions};
use std::collections::BTreeMap;
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-vec-index-meta-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn clear_normalized_sidecars(cassie: &Cassie, collection: &str, field: &str) {
    let prefix = format!("__cassie__/normalized-vector/{collection}/{field}/");
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, prefix.as_bytes())
        .unwrap();
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    for (key, _value) in entries {
        tx.delete(key).unwrap();
    }
    tx.commit(WriteOptions::sync()).unwrap();
}

#[test]
fn should_persist_vector_index_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("persist");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async move {
        // Act
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "index_meta_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
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

        let record = VectorIndexRecord {
            collection: collection.to_string(),
            field: "embedding".to_string(),
            source_field: "content".to_string(),
            metadata: VectorIndexMetadata {
                provider: "openai".to_string(),
                model: "text-embedding-3-small".to_string(),
                dimensions: 3,
                metric: DistanceMetric::Cosine,
            },
        };

        cassie.midge.put_vector_index(record.clone()).unwrap();

        let loaded = cassie
            .midge
            .get_vector_index(collection, "embedding")
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(loaded, record);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_reload_registry_after_restart_simulation() {
    // Arrange
    with_fallback();
    let path = data_dir("restart");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async move {
        // Act
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "restart_index_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "text".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "vector".to_string(),
                    data_type: DataType::Vector(2),
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

        let record = VectorIndexRecord {
            collection: collection.to_string(),
            field: "vector".to_string(),
            source_field: "text".to_string(),
            metadata: VectorIndexMetadata {
                provider: "voyage".to_string(),
                model: "voyage-3-large".to_string(),
                dimensions: 2,
                metric: DistanceMetric::L2,
            },
        };

        cassie.midge.put_vector_index(record.clone()).unwrap();
        let before_restart = cassie
            .midge
            .list_vector_indexes()
            .expect("vector indexes before restart");
        assert_eq!(before_restart.len(), 1);
        assert_eq!(before_restart[0], record);

        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();

        let stored = restarted
            .midge
            .list_vector_indexes()
            .expect("stored vector index records");
        assert!(!stored.is_empty());

        let hydrated = restarted
            .catalog
            .get_vector_index(collection, "vector")
            .unwrap();

        // Assert
        assert_eq!(hydrated, record);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_rebuild_missing_normalized_sidecars_on_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("normalized_restart");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async move {
        // Act
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "normalized_restart_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "body".to_string(),
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

        let record = VectorIndexRecord {
            collection: collection.to_string(),
            field: "embedding".to_string(),
            source_field: "body".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 3,
                metric: DistanceMetric::Cosine,
            },
        };
        cassie.midge.put_vector_index(record.clone()).unwrap();

        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({
                    "body": "alpha",
                    "embedding": [3.0, 4.0, 0.0],
                }),
            )
            .unwrap();

        let stored = cassie
            .midge
            .get_normalized_vector(collection, "embedding", "doc-1")
            .unwrap()
            .unwrap();
        assert_eq!(stored.values, vec![0.6, 0.8, 0.0]);

        clear_normalized_sidecars(&cassie, collection, "embedding");
        assert!(cassie
            .midge
            .get_normalized_vector(collection, "embedding", "doc-1")
            .unwrap()
            .is_none());

        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();

        let rebuilt = restarted
            .midge
            .get_normalized_vector(collection, "embedding", "doc-1")
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(rebuilt.collection, collection);
        assert_eq!(rebuilt.field, "embedding");
        assert_eq!(rebuilt.id, "doc-1");
        assert_eq!(rebuilt.dimensions, 3);
        assert_eq!(rebuilt.metric, DistanceMetric::Cosine);
        assert!(rebuilt.payload_available);
        assert_eq!(rebuilt.normalization_version, 1);
        assert_eq!(rebuilt.values, vec![0.6, 0.8, 0.0]);
        assert_eq!(rebuilt.magnitude, 5.0);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_reject_normalized_sidecar_rebuild_when_index_dimensions_do_not_match_document_values() {
    // Arrange
    with_fallback();
    let path = data_dir("normalized_dimension_mismatch");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async move {
        // Act
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "normalized_dimension_mismatch_docs";
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
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );

        let record = VectorIndexRecord {
            collection: collection.to_string(),
            field: "embedding".to_string(),
            source_field: "embedding".to_string(),
            metadata: VectorIndexMetadata {
                provider: "manual".to_string(),
                model: "manual".to_string(),
                dimensions: 4,
                metric: DistanceMetric::Cosine,
            },
        };
        cassie.midge.put_vector_index(record).unwrap();

        let result = cassie.midge.put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({
                "embedding": [1.0, 2.0, 3.0],
            }),
        );

        // Assert
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("expects 4 dimensions"));
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_reload_generic_index_registry_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("generic_index_restart");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async move {
        // Act
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "generic_index_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
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
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect(),
        );

        let record = IndexMeta {
            collection: collection.to_string(),
            name: "idx_generic_title".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            include_fields: Vec::new(),
            kind: IndexKind::Scalar,
            unique: true,
            options: BTreeMap::from_iter(vec![("case_sensitive".to_string(), "true".to_string())]),
        };
        cassie.midge.put_index(record.clone()).unwrap();

        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();

        // Assert
        let loaded = restarted
            .catalog
            .get_index(collection, "idx_generic_title")
            .expect("index should hydrate");
        assert_eq!(loaded, record);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_persist_fulltext_index_metadata_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("fulltext_index_restart");
    let path_for_cleanup = path.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async move {
        // Act
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "fulltext_restart_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
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

        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect(),
        );

        let expected = IndexMeta {
            collection: collection.to_string(),
            name: "idx_fulltext_body".to_string(),
            field: "body".to_string(),
            fields: vec!["body".to_string()],
            include_fields: Vec::new(),
            kind: IndexKind::FullText,
            unique: false,
            options: BTreeMap::from_iter(vec![
                ("boost".to_string(), "2".to_string()),
                ("k1".to_string(), "0.7".to_string()),
                ("b".to_string(), "0.2".to_string()),
            ]),
        };
        cassie.midge.put_index(expected.clone()).unwrap();

        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();

        // Assert
        let loaded = restarted
            .catalog
            .get_index(collection, "idx_fulltext_body")
            .expect("index should hydrate");
        assert_eq!(loaded, expected);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}
