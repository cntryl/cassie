use super::{
    Arc, Cassie, CassieError, CassieRuntimeConfig, CohereProvider, DistanceMetric, Embedding,
    EmbeddingsRuntimeConfig, LocalProvider, OllamaProvider, OllamaProviderConfig,
    OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig, OpenAiCompatibleRuntimeConfig,
    OpenAiProvider, OpenAiProviderConfig, OpenAiRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
    TeiProvider, TeiProviderConfig, VectorIndexRecord, VoyageProvider,
};
use crate::embeddings::EmbeddingProvider;

#[derive(Debug, Default)]
struct DisabledProvider;

impl EmbeddingProvider for DisabledProvider {
    fn provider_name(&self) -> &'static str {
        "disabled"
    }

    fn model_name(&self) -> &'static str {
        "disabled"
    }

    fn dimensions(&self) -> usize {
        0
    }

    fn embed_documents(
        &self,
        _inputs: &[String],
    ) -> Result<Vec<Embedding>, crate::embeddings::EmbeddingError> {
        Err(crate::embeddings::EmbeddingError::Unavailable {
            provider: self.provider_name().to_string(),
            reason: "embedding provider is disabled".to_string(),
        })
    }

    fn embed_query(&self, _input: &str) -> Result<Embedding, crate::embeddings::EmbeddingError> {
        Err(crate::embeddings::EmbeddingError::Unavailable {
            provider: self.provider_name().to_string(),
            reason: "embedding provider is disabled".to_string(),
        })
    }
}

pub(super) fn build_embedding_provider(
    config: &CassieRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let max_response_bytes = config.embeddings_max_response_bytes;
    match &config.embeddings {
        EmbeddingsRuntimeConfig::Disabled => Ok(Arc::new(DisabledProvider)),
        EmbeddingsRuntimeConfig::Voyage(runtime) => {
            build_voyage_provider(runtime, max_response_bytes)
        }
        EmbeddingsRuntimeConfig::Cohere(runtime) => {
            build_cohere_provider(runtime, max_response_bytes)
        }
        EmbeddingsRuntimeConfig::Local(runtime) => build_local_provider(runtime),
        EmbeddingsRuntimeConfig::OpenAI(runtime) => {
            build_openai_provider(runtime, max_response_bytes)
        }
        EmbeddingsRuntimeConfig::OpenAiCompatible(runtime) => {
            build_openai_compatible_provider(runtime, max_response_bytes)
        }
        EmbeddingsRuntimeConfig::Tei(runtime) => build_tei_provider(runtime, max_response_bytes),
        EmbeddingsRuntimeConfig::Ollama(runtime) => {
            build_ollama_provider(runtime, max_response_bytes)
        }
    }
}

fn build_openai_provider(
    runtime: &OpenAiRuntimeConfig,
    max_response_bytes: usize,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let config = OpenAiProviderConfig {
        api_key: runtime.config.api_key.clone(),
        model: runtime.config.model.clone(),
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
        base_url: runtime
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com".to_string()),
    };

    let provider = OpenAiProvider::with_config(config)?.with_max_response_bytes(max_response_bytes);
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_voyage_provider(
    runtime: &super::VoyageRuntimeConfig,
    max_response_bytes: usize,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = VoyageProvider::with_config(super::VoyageProviderConfig {
        api_key: runtime.api_key.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
        base_url: runtime.base_url.clone(),
    })?
    .with_max_response_bytes(max_response_bytes);
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_cohere_provider(
    runtime: &super::CohereRuntimeConfig,
    max_response_bytes: usize,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = CohereProvider::with_config(super::CohereProviderConfig {
        api_key: runtime.api_key.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
        base_url: runtime.base_url.clone(),
    })?
    .with_max_response_bytes(max_response_bytes);
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_local_provider(
    runtime: &super::LocalRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = LocalProvider::with_config(super::LocalProviderConfig {
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
    })?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_openai_compatible_provider(
    runtime: &OpenAiCompatibleRuntimeConfig,
    max_response_bytes: usize,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
        api_key: runtime.api_key.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
        base_url: runtime.base_url.clone(),
    })?
    .with_max_response_bytes(max_response_bytes);
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_tei_provider(
    runtime: &SelfHostedEmbeddingRuntimeConfig,
    max_response_bytes: usize,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: runtime.base_url.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
    })?
    .with_max_response_bytes(max_response_bytes);
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_ollama_provider(
    runtime: &SelfHostedEmbeddingRuntimeConfig,
    max_response_bytes: usize,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = OllamaProvider::with_config(OllamaProviderConfig {
        base_url: runtime.base_url.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
    })?
    .with_max_response_bytes(max_response_bytes);
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

impl Cassie {
    pub(crate) fn validate_embedding_compatibility(
        &self,
        index: &VectorIndexRecord,
        requested_metric: Option<&DistanceMetric>,
    ) -> Result<(), CassieError> {
        if self.embedding_provider.provider_name() != index.metadata.provider {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding provider mismatch: index requires '{}', active is '{}'",
                index.metadata.provider,
                self.embedding_provider.provider_name()
            )));
        }

        if self.embedding_provider.model_name() != index.metadata.model {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding model mismatch: index requires '{}', active is '{}'",
                index.metadata.model,
                self.embedding_provider.model_name()
            )));
        }

        if self.embedding_provider.dimensions() != index.metadata.dimensions {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding dimension mismatch: index requires {}, active provider has {}",
                index.metadata.dimensions,
                self.embedding_provider.dimensions()
            )));
        }

        if let Some(metric) = requested_metric {
            if *metric != index.metadata.metric {
                return Err(CassieError::InvalidEmbedding(format!(
                    "embedding metric mismatch: index requires '{}', request requested '{}'",
                    index.metadata.metric.as_str(),
                    metric.as_str()
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn validate_embedding_payload(
        index: &VectorIndexRecord,
        embedding: &Embedding,
    ) -> Result<(), CassieError> {
        if embedding.values.len() != index.metadata.dimensions {
            return Err(CassieError::InvalidEmbedding(format!(
                "embedding dimension mismatch: index requires {} and got {}",
                index.metadata.dimensions,
                embedding.values.len()
            )));
        }

        Ok(())
    }
}
