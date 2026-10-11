use cassie::app::Cassie;
use cassie::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};
use cassie::runtime::QueryExecutionControls;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub struct CountingEmbeddingProvider {
    inner: Arc<dyn EmbeddingProvider>,
    calls: AtomicUsize,
}

impl CountingEmbeddingProvider {
    pub fn install(cassie: &mut Cassie) -> Arc<Self> {
        let provider = Arc::new(Self {
            inner: Arc::clone(&cassie.embedding_provider),
            calls: AtomicUsize::new(0),
        });
        cassie.embedding_provider = provider.clone();
        provider
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub fn reset_calls(&self) {
        self.calls.store(0, Ordering::SeqCst);
    }
}

impl EmbeddingProvider for CountingEmbeddingProvider {
    fn provider_name(&self) -> &'static str {
        self.inner.provider_name()
    }

    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn dimensions(&self) -> usize {
        self.inner.dimensions()
    }

    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.embed_documents(inputs)
    }

    fn embed_documents_with_controls(
        &self,
        inputs: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.embed_documents_with_controls(inputs, controls)
    }

    fn embed_query(&self, input: &str) -> Result<Embedding, EmbeddingError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.embed_query(input)
    }

    fn embed_query_with_controls(
        &self,
        input: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Embedding, EmbeddingError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.embed_query_with_controls(input, controls)
    }
}
