use cassie::app::Cassie;
use cassie::embeddings::{DistanceMetric, VectorIndexMetadata, VectorIndexRecord};
use cassie::types::{DataType, FieldSchema, Schema};
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
        cassie.startup().await.unwrap();

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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

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

        cassie.midge.put_vector_index(record.clone()).await.unwrap();

        let loaded = cassie
            .midge
            .get_vector_index(collection, "embedding")
            .await
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
        cassie.startup().await.unwrap();

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
            .await
            .unwrap();
        cassie
            .register_collection(
                collection,
                schema
                    .fields
                    .iter()
                    .map(|field| (field.name.clone(), field.data_type.clone()))
                    .collect(),
            )
            .await;

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

        cassie.midge.put_vector_index(record.clone()).await.unwrap();
        let before_restart = cassie
            .midge
            .list_vector_indexes()
            .await
            .expect("vector indexes before restart");
        assert_eq!(before_restart.len(), 1);
        assert_eq!(before_restart[0], record);

        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().await.unwrap();

        let stored = restarted
            .midge
            .list_vector_indexes()
            .await
            .expect("stored vector index records");
        assert!(!stored.is_empty());

        let hydrated = restarted
            .catalog
            .get_vector_index(collection, "vector")
            .await
            .unwrap();

        // Assert
        assert_eq!(hydrated, record);
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}
