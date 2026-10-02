use std::sync::atomic::{AtomicUsize, Ordering};

use cassie::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};

#[derive(Debug, Default)]
pub struct FailSecondProvider(AtomicUsize);

impl EmbeddingProvider for FailSecondProvider {
    fn provider_name(&self) -> &'static str {
        "local"
    }
    fn model_name(&self) -> &'static str {
        "deterministic-test"
    }
    fn dimensions(&self) -> usize {
        3
    }
    fn embed_documents(&self, _: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(vec![Embedding {
                values: vec![1.0, 0.0, 0.0],
            }])
        } else {
            Err(EmbeddingError::InvalidConfiguration(
                "injected second embedding failure".to_string(),
            ))
        }
    }
}

impl FailSecondProvider {
    pub fn call_count(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}
