use crate::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

#[derive(Debug, Default)]
pub struct LocalProvider;

impl EmbeddingProvider for LocalProvider {
    fn provider_name(&self) -> &'static str {
        "local"
    }

    fn model_name(&self) -> &str {
        "stub"
    }

    fn dimensions(&self) -> usize {
        0
    }

    fn embed_documents(&self, _inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        Err(EmbeddingError::NotImplemented {
            provider: self.provider_name().to_string(),
        })
    }
}
