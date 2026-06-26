use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-embed-val-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn openai_runtime(base_url: String) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
        config: OpenAiConfig {
            api_key: "test-key".to_string(),
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
        },
        timeout_seconds: 1,
        max_batch_size: 1,
        max_retries: 1,
        base_url: Some(base_url),
    });
    config
}

fn voyage_runtime() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Voyage;
    config
}

async fn ensure_collection(cassie: &Cassie, collection: &str, schema: Schema) {
    // Arrange.
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

fn vector_index_record(
    collection: &str,
    provider: &str,
    dimensions: usize,
    metric: DistanceMetric,
) -> VectorIndexRecord {
    VectorIndexRecord {
        collection: collection.to_string(),
        field: "embedding".to_string(),
        source_field: "content".to_string(),
        metadata: VectorIndexMetadata {
            provider: provider.to_string(),
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
            dimensions,
            metric,
            index_type: VectorIndexType::BruteForce,
            hnsw: None,
            ivfflat: None,
            ivfflat_training: None,
        },
    }
}

#[test]
fn should_reject_ingest_when_query_provider_model_mismatch() {
    // Arrange
    with_fallback();
    let path = data_dir("provider_mismatch");
    let path_for_cleanup = path.clone();
    let openai = Cassie::new_with_data_dir_and_config(
        &path,
        openai_runtime("http://127.0.0.1:1".to_string()),
    )
    .unwrap();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        // Arrange (seed existing index metadata)
        let collection = "provider_mismatch_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(1536),
                    nullable: true,
                },
            ],
        };

        openai.startup().unwrap();
        ensure_collection(&openai, collection, schema.clone()).await;

        openai
            .midge
            .put_vector_index(vector_index_record(
                collection,
                "openai",
                1536,
                DistanceMetric::Cosine,
            ))
            .unwrap();
        openai.register_vector_index(vector_index_record(
            collection,
            "openai",
            1536,
            DistanceMetric::Cosine,
        ));
    });

    drop(openai);

    let voyage = Cassie::new_with_data_dir_and_config(&path, voyage_runtime()).unwrap();
    runtime.block_on(async {
        // Act
        voyage.startup().unwrap();
        let result = voyage.ingest_document(
            "provider_mismatch_docs",
            serde_json::json!({"content": "sample text"}),
        );

        // Assert
        assert!(matches!(result, Err(CassieError::InvalidEmbedding(_))));
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_reject_ingest_when_dimensions_change() {
    // Arrange
    with_fallback();
    let path = data_dir("dimension_mismatch");
    let path_for_cleanup = path.clone();
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        openai_runtime("http://127.0.0.1:1".to_string()),
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        // Act
        let collection = "dimension_mismatch_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(1536),
                    nullable: true,
                },
            ],
        };

        cassie.startup().unwrap();
        ensure_collection(&cassie, collection, schema.clone()).await;
        let index_record = vector_index_record(collection, "openai", 2, DistanceMetric::Cosine);
        cassie.midge.put_vector_index(index_record.clone()).unwrap();
        cassie.register_vector_index(index_record);

        // Assert
        let result =
            cassie.ingest_document(collection, serde_json::json!({"content": "sample text"}));
        assert!(matches!(result, Err(CassieError::InvalidEmbedding(_))));
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}

#[test]
fn should_reject_query_when_metric_different() {
    // Arrange
    with_fallback();
    let path = data_dir("metric_mismatch");
    let path_for_cleanup = path.clone();
    let cassie = Cassie::new_with_data_dir_and_config(
        &path,
        openai_runtime("http://127.0.0.1:1".to_string()),
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        // Act
        let collection = "metric_mismatch_docs";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "content".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "embedding".to_string(),
                    data_type: DataType::Vector(1536),
                    nullable: true,
                },
            ],
        };

        cassie.startup().unwrap();
        ensure_collection(&cassie, collection, schema).await;
        let index_record = vector_index_record(collection, "openai", 1536, DistanceMetric::Cosine);
        cassie.midge.put_vector_index(index_record.clone()).unwrap();
        cassie.register_vector_index(index_record);

        // Assert
        let result = cassie.execute_vector_search(
            collection,
            "embedding",
            "query",
            Some(DistanceMetric::Dot),
            10,
            0,
        );
        assert!(matches!(result, Err(CassieError::InvalidEmbedding(_))));
    });

    let _ = std::fs::remove_dir_all(path_for_cleanup);
}
