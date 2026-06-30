use super::{
    Arc, Cassie, CassieError, CassieRuntimeConfig, CohereProvider, DistanceMetric, Embedding,
    EmbeddingsRuntimeConfig, LocalProvider, OllamaProvider, OllamaProviderConfig,
    OpenAiCompatibleProvider, OpenAiCompatibleProviderConfig, OpenAiCompatibleRuntimeConfig,
    OpenAiProvider, OpenAiProviderConfig, OpenAiRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
    TeiProvider, TeiProviderConfig, VectorIndexRecord, VoyageProvider,
};
use crate::embeddings::EmbeddingProvider;

pub(super) fn build_embedding_provider(
    config: &CassieRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    match &config.embeddings {
        EmbeddingsRuntimeConfig::Voyage => Ok(Arc::new(VoyageProvider)),
        EmbeddingsRuntimeConfig::Cohere => Ok(Arc::new(CohereProvider)),
        EmbeddingsRuntimeConfig::Disabled | EmbeddingsRuntimeConfig::Local => {
            Ok(Arc::new(LocalProvider))
        }
        EmbeddingsRuntimeConfig::OpenAI(runtime) => build_openai_provider(runtime),
        EmbeddingsRuntimeConfig::OpenAiCompatible(runtime) => {
            build_openai_compatible_provider(runtime)
        }
        EmbeddingsRuntimeConfig::Tei(runtime) => build_tei_provider(runtime),
        EmbeddingsRuntimeConfig::Ollama(runtime) => build_ollama_provider(runtime),
    }
}

fn build_openai_provider(
    runtime: &OpenAiRuntimeConfig,
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

    let provider = OpenAiProvider::with_config(config)?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_openai_compatible_provider(
    runtime: &OpenAiCompatibleRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = OpenAiCompatibleProvider::with_config(OpenAiCompatibleProviderConfig {
        api_key: runtime.api_key.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
        base_url: runtime.base_url.clone(),
    })?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_tei_provider(
    runtime: &SelfHostedEmbeddingRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = TeiProvider::with_config(TeiProviderConfig {
        base_url: runtime.base_url.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
    })?;
    Ok(Arc::new(provider) as Arc<dyn EmbeddingProvider>)
}

fn build_ollama_provider(
    runtime: &SelfHostedEmbeddingRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    let provider = OllamaProvider::with_config(OllamaProviderConfig {
        base_url: runtime.base_url.clone(),
        model: runtime.model.clone(),
        dimensions: runtime.dimensions,
        timeout: std::time::Duration::from_secs(runtime.timeout_seconds),
        max_batch_size: runtime.max_batch_size,
        max_retries: runtime.max_retries,
    })?;
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
