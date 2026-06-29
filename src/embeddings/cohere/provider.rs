use crate::embeddings::EmbeddingProvider;
use crate::embeddings::{Embedding, EmbeddingError};

#[derive(Debug, Default)]
pub struct CohereProvider;

impl EmbeddingProvider for CohereProvider {
    fn provider_name(&self) -> &'static str {
        "cohere"
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
