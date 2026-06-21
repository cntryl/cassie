use super::*;

pub(super) fn build_embedding_provider(
    config: &CassieRuntimeConfig,
) -> Result<Arc<dyn EmbeddingProvider>, CassieError> {
    match &config.embeddings {
        EmbeddingsRuntimeConfig::Disabled => Ok(Arc::new(LocalProvider)),
        EmbeddingsRuntimeConfig::Voyage => Ok(Arc::new(VoyageProvider)),
        EmbeddingsRuntimeConfig::Cohere => Ok(Arc::new(CohereProvider)),
        EmbeddingsRuntimeConfig::Local => Ok(Arc::new(LocalProvider)),
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
