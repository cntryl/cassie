use sha2::{Digest, Sha256};

use crate::EmbeddingProvider;
use crate::{Embedding, EmbeddingError};
use cassie_core::request::Interruption;

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

/// Domain bytes are part of the deterministic local embedding format.
#[derive(Clone, Copy)]
enum EmbeddingDomain {
    Document,
    Query,
}

impl EmbeddingDomain {
    const fn label(self) -> &'static [u8] {
        match self {
            Self::Document => b"document",
            Self::Query => b"query",
        }
    }
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

    fn derive_embedding(&self, input: &str, domain: EmbeddingDomain) -> Embedding {
        let mut values = Vec::with_capacity(self.dimensions);
        let mut counter = 0u64;

        while values.len() < self.dimensions {
            let mut hasher = Sha256::new();
            hasher.update(self.model.as_bytes());
            hasher.update([0]);
            hasher.update(domain.label());
            hasher.update([0]);
            hasher.update(input.as_bytes());
            hasher.update(counter.to_be_bytes());
            let digest = hasher.finalize();

            for chunk in digest.as_chunks::<4>().0 {
                if values.len() == self.dimensions {
                    break;
                }
                let raw = u32::from_be_bytes(*chunk);
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
            .map(|input| self.derive_embedding(input, EmbeddingDomain::Document))
            .collect())
    }

    fn embed_query(&self, input: &str) -> Result<Embedding, EmbeddingError> {
        Ok(self.derive_embedding(input, EmbeddingDomain::Query))
    }

    fn embed_query_with_interruption(
        &self,
        input: &str,
        interruption: Interruption<'_>,
    ) -> Result<Embedding, EmbeddingError> {
        crate::provider::check_controls(self.provider_name(), interruption)?;
        let embedding = self.embed_query(input)?;
        crate::provider::check_controls(self.provider_name(), interruption)?;
        Ok(embedding)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_preserve_deterministic_local_embedding_domains() {
        // Arrange
        let provider = LocalProvider::with_config(LocalProviderConfig {
            model: "compat-model".to_owned(),
            dimensions: 8,
        })
        .unwrap();
        let input = "same input";
        let expected = [
            [
                -0.438_204, -0.376_842, -0.429_595, 0.140_897, 0.311_611, -0.050_743, -0.423_699,
                0.427_044,
            ],
            [
                -0.368_898, -0.596_904, 0.025_896, 0.146_161, -0.414_255, 0.334_858, -0.403_034,
                -0.198_527,
            ],
        ];

        // Act
        let documents = provider.embed_documents(&[input.to_owned()]).unwrap();
        let query = provider.embed_query(input).unwrap();

        // Assert
        for (embedding, expected) in [&documents[0], &query].into_iter().zip(expected) {
            assert_eq!(embedding.values.len(), expected.len());
            for (actual, expected) in embedding.values.iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 0.000_002,
                    "actual {actual}, expected {expected}"
                );
            }
        }
    }
}
