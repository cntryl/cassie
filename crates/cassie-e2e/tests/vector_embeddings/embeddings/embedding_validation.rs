use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::support_sql as support;
use cassie::app::{Cassie, CassieError};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig, OpenAiRuntimeConfig,
    VoyageRuntimeConfig,
};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, Embedding, EmbeddingError, EmbeddingProvider,
    Interruption, VectorIndexMetadata, VectorIndexRecord, VectorIndexType, DEFAULT_EMBEDDING_MODEL,
};
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use support::*;

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
    config.embeddings = EmbeddingsRuntimeConfig::Voyage(VoyageRuntimeConfig {
        api_key: "voyage-test-key".to_string(),
        model: "voyage-3.5-lite".to_string(),
        dimensions: 1024,
        timeout_seconds: 1,
        max_batch_size: 1,
        max_retries: 1,
        base_url: "http://127.0.0.1:1".to_string(),
    });
    config
}

fn ensure_collection(cassie: &Cassie, collection: &str, schema: &Schema) {
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
            hnsw_graph: None,
            ivfflat: None,
            ivfflat_training: None,
        },
    }
}

#[test]
fn should_reject_ingest_when_query_provider_model_mismatch() {
    // Arrange
    use_local_storage();
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
                    data_type: DataType::Vector(Some(1536)),
                    nullable: true,
                },
            ],
        };

        openai.startup().unwrap();
        ensure_collection(&openai, collection, &schema);

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
    crate::support_sql::install_vector_extension(&voyage);
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
    let cassie =
        Cassie::new_in_memory_with_config(openai_runtime("http://127.0.0.1:1".to_string()))
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
                    data_type: DataType::Vector(Some(1536)),
                    nullable: true,
                },
            ],
        };

        cassie.startup().unwrap();
        ensure_collection(&cassie, collection, &schema);
        let index_record = vector_index_record(collection, "openai", 2, DistanceMetric::Cosine);
        cassie.midge.put_vector_index(index_record.clone()).unwrap();
        cassie.register_vector_index(index_record);

        // Assert
        let result =
            cassie.ingest_document(collection, serde_json::json!({"content": "sample text"}));
        assert!(matches!(result, Err(CassieError::InvalidEmbedding(_))));
    });
}

#[test]
fn should_reject_query_when_metric_different() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(openai_runtime("http://127.0.0.1:1".to_string()))
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
                    data_type: DataType::Vector(Some(1536)),
                    nullable: true,
                },
            ],
        };

        cassie.startup().unwrap();
        ensure_collection(&cassie, collection, &schema);
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
}

struct FixtureEmbeddingProvider {
    document: [f32; 3],
    first_query: [f32; 3],
    later_query: [f32; 3],
    document_calls: AtomicUsize,
    query_calls: AtomicUsize,
}

impl FixtureEmbeddingProvider {
    fn new(document: [f32; 3], first_query: [f32; 3], later_query: [f32; 3]) -> Self {
        Self {
            document,
            first_query,
            later_query,
            document_calls: AtomicUsize::new(0),
            query_calls: AtomicUsize::new(0),
        }
    }
}

impl EmbeddingProvider for FixtureEmbeddingProvider {
    fn provider_name(&self) -> &'static str {
        "local"
    }

    fn model_name(&self) -> &'static str {
        "role-fixture"
    }

    fn dimensions(&self) -> usize {
        3
    }

    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        self.document_calls.fetch_add(1, Ordering::Relaxed);
        assert_eq!(inputs, &["alpha".to_owned()]);
        Ok(vec![Embedding {
            values: self.document.to_vec(),
        }])
    }

    fn embed_query(&self, input: &str) -> Result<Embedding, EmbeddingError> {
        assert_eq!(input, "alpha");
        let values = if self.query_calls.fetch_add(1, Ordering::Relaxed) == 0 {
            self.first_query
        } else {
            self.later_query
        };
        Ok(Embedding {
            values: values.to_vec(),
        })
    }

    fn embed_query_with_interruption(
        &self,
        input: &str,
        _interruption: Interruption<'_>,
    ) -> Result<Embedding, EmbeddingError> {
        self.embed_query(input)
    }
}

fn indexed_embedding_fixture(provider: Arc<FixtureEmbeddingProvider>) -> Cassie {
    let config = CassieRuntimeConfig {
        embeddings: EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "role-fixture".to_string(),
            dimensions: 3,
        }),
        ..CassieRuntimeConfig::default()
    };
    let mut cassie = Cassie::new_in_memory_with_config(config).expect("embedding fixture");
    crate::support_sql::install_vector_extension(&cassie);
    cassie.embedding_provider = provider;
    cassie.startup().expect("start fixture");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE role_docs (id TEXT, body TEXT, embedding VECTOR(3))",
        "CREATE INDEX role_docs_vec ON role_docs USING vector (embedding) WITH (source_field=body, metric=dot, index_type=bruteforce)",
    ] {
        cassie.execute_sql(&session, statement, vec![]).expect("index setup");
    }
    cassie
}

#[test]
fn should_dispatch_each_embedding_request_to_its_provider_role() {
    // Arrange
    let document = [1.0, 0.0, 0.0];
    for query in [[0.0, 1.0, 0.0], document] {
        let provider = Arc::new(FixtureEmbeddingProvider::new(document, query, query));
        let cassie = indexed_embedding_fixture(Arc::clone(&provider));
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO role_docs (id, body) VALUES ('a', 'alpha')",
                vec![],
            )
            .expect("SQL document write");
        let stored = cassie
            .execute_sql(&session, "SELECT embedding FROM role_docs", vec![])
            .expect("read stored embedding");
        let document_calls = provider.document_calls.load(Ordering::Relaxed);
        let query_calls = provider.query_calls.load(Ordering::Relaxed);
        let found = cassie
            .execute_vector_search("role_docs", "embedding", "alpha", None, 1, 0)
            .expect("query embedding");

        // Assert
        assert_eq!(
            stored.rows(),
            &[vec![Value::Vector(Vector::new(document.to_vec()))]]
        );
        assert_eq!(document_calls, 1);
        assert_eq!(query_calls, 0);
        assert_eq!(found.rows().len(), 1);
        assert_eq!(provider.document_calls.load(Ordering::Relaxed), 1);
        assert_eq!(provider.query_calls.load(Ordering::Relaxed), 1);
    }
}

#[test]
fn should_reject_nonfinite_query_embeddings_before_cache_admission() {
    // Arrange
    let valid = [1.0, 0.0, 0.0];
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let provider = Arc::new(FixtureEmbeddingProvider::new(
            valid,
            [invalid, 0.0, 0.0],
            valid,
        ));
        let cassie = indexed_embedding_fixture(Arc::clone(&provider));
        cassie
            .execute_sql(
                &cassie.create_session("tester", None),
                "INSERT INTO role_docs VALUES ('a', 'alpha', '[1,0,0]')",
                vec![],
            )
            .expect("seed a candidate with an explicit finite vector");
        let query = || cassie.execute_vector_search("role_docs", "embedding", "alpha", None, 1, 0);

        // Act
        let invalid_result = query();
        let calls_after_invalid = provider.query_calls.load(Ordering::Relaxed);
        let recovered = query();
        let calls_after_recovery = provider.query_calls.load(Ordering::Relaxed);
        let cached = query();

        // Assert
        assert!(matches!(
            invalid_result,
            Err(CassieError::InvalidEmbedding(_))
        ));
        assert_eq!(calls_after_invalid, 1);
        assert_eq!(
            recovered
                .expect("retry fetched a finite vector")
                .rows()
                .len(),
            1
        );
        assert_eq!(calls_after_recovery, 2);
        assert_eq!(cached.expect("valid vector is reusable").rows().len(), 1);
        assert_eq!(provider.query_calls.load(Ordering::Relaxed), 2);
    }
}
