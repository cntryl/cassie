use sha2::{Digest, Sha256};

use crate::embeddings::EmbeddingProvider;
use crate::embeddings::{Embedding, EmbeddingError};

#[derive(Debug, Clone)]
pub struct LocalProviderConfig {
    pub model: String,
    pub dimensions: usize,
}

#[derive(Debug)]
pub struct LocalProvider {
    model: String,
    dimensions: usize,
}

impl LocalProvider {
    /// # Errors
    ///
    /// Returns an error when the provider configuration is invalid.
    pub fn with_config(config: LocalProviderConfig) -> Result<Self, EmbeddingError> {
        if config.model.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "local embedding model is required".to_string(),
            ));
        }
        if config.dimensions == 0 {
            return Err(EmbeddingError::InvalidConfiguration(
                "local embedding dimensions must be greater than zero".to_string(),
            ));
        }

        Ok(Self {
            model: config.model,
            dimensions: config.dimensions,
        })
    }

    fn derive_embedding(&self, input: &str, salt: &str) -> Embedding {
        let mut values = Vec::with_capacity(self.dimensions);
        let mut counter = 0u64;

        while values.len() < self.dimensions {
            let mut hasher = Sha256::new();
            hasher.update(self.model.as_bytes());
            hasher.update([0]);
            hasher.update(salt.as_bytes());
            hasher.update([0]);
            hasher.update(input.as_bytes());
            hasher.update(counter.to_be_bytes());
            let digest = hasher.finalize();

            for chunk in digest.chunks_exact(4) {
                if values.len() == self.dimensions {
                    break;
                }
                let bytes: [u8; 4] = chunk.try_into().expect("sha256 digest chunk");
                let raw = u32::from_be_bytes(bytes);
                let scaled = f32::from_bits(0x3F80_0000 | (raw >> 9)) - 1.0;
                values.push((scaled * 2.0) - 1.0);
            }

            counter = counter.saturating_add(1);
        }

        normalize(&mut values);
        Embedding { values }
    }
}

impl EmbeddingProvider for LocalProvider {
    fn provider_name(&self) -> &'static str {
        "local"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        Ok(inputs
            .iter()
            .map(|input| self.derive_embedding(input, "document"))
            .collect())
    }

    fn embed_query(&self, input: &str) -> Result<Embedding, EmbeddingError> {
        Ok(self.derive_embedding(input, "query"))
    }
}

fn normalize(values: &mut [f32]) {
    let magnitude = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if magnitude <= f32::EPSILON {
        if let Some(first) = values.first_mut() {
            *first = 1.0;
        }
        for value in values.iter_mut().skip(1) {
            *value = 0.0;
        }
        return;
    }

    for value in values {
        *value /= magnitude;
    }
}
