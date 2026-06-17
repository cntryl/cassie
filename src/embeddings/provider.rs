use crate::embeddings::{Embedding, EmbeddingError};

pub trait EmbeddingProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    fn model_name(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError>;
    fn embed_query(&self, input: &str) -> Result<Embedding, EmbeddingError> {
        self.embed_documents(std::slice::from_ref(&input.to_string()))
            .map(|batch| {
                batch
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| Embedding { values: Vec::new() })
            })
    }
}
