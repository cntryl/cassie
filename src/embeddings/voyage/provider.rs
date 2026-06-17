use crate::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

#[derive(Debug, Default)]
pub struct VoyageProvider;

impl EmbeddingProvider for VoyageProvider {
    fn provider_name(&self) -> &'static str {
        "voyage"
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
