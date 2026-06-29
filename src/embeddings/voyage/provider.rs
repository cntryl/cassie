use crate::embeddings::EmbeddingProvider;
use crate::embeddings::{Embedding, EmbeddingError};

#[derive(Debug, Default)]
pub struct VoyageProvider;

impl EmbeddingProvider for VoyageProvider {
    fn provider_name(&self) -> &'static str {
        "voyage"
    }

    fn model_name(&self) -> &'static str {
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
