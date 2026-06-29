use crate::embeddings::EmbeddingProvider;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::Serialize;

use crate::embeddings::{Embedding, EmbeddingError};

#[derive(Debug, Clone)]
pub struct TeiProviderConfig {
    pub base_url: String,
    pub model: String,
    pub dimensions: usize,
    pub timeout: Duration,
    pub max_batch_size: usize,
    pub max_retries: usize,
}

#[derive(Debug)]
pub struct TeiProvider {
    base_url: String,
    model: String,
    dimensions: usize,
    client: Client,
    max_batch_size: usize,
    max_retries: usize,
}

#[derive(Debug, Serialize, Clone)]
struct TeiRequest {
    inputs: Vec<String>,
}

impl TeiProvider {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn with_config(config: TeiProviderConfig) -> Result<Self, EmbeddingError> {
        if config.base_url.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "TEI base URL is required".to_string(),
            ));
        }
        if config.model.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "TEI model is required".to_string(),
            ));
        }
        if config.dimensions == 0 {
            return Err(EmbeddingError::InvalidConfiguration(
                "TEI dimensions must be greater than zero".to_string(),
            ));
        }

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|error| {
                EmbeddingError::InvalidConfiguration(format!(
                    "failed to configure TEI client: {error}"
                ))
            })?;

        Ok(Self {
            base_url: config.base_url,
            model: config.model,
            dimensions: config.dimensions,
            client,
            max_batch_size: config.max_batch_size.max(1),
            max_retries: config.max_retries.max(1),
        })
    }

    fn embed_documents_batch(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let request = TeiRequest {
            inputs: inputs.to_vec(),
        };
        let endpoint = format!("{}/embed", self.base_url.trim_end_matches('/'));

        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let client = self.client.clone();
            let endpoint = endpoint.clone();
            let request_snapshot = request.clone();
            let response = self.run_blocking(move || {
                let response = client.post(endpoint).json(&request_snapshot).send()?;
                let status = response.status();
                let body = response.text()?;
                Ok::<_, reqwest::Error>((status, body))
            })?;

            match response {
                Ok((status, body)) if status.is_success() => {
                    let vectors: Vec<Vec<f32>> = serde_json::from_str(&body).map_err(|error| {
                        EmbeddingError::ParseError(format!("TEI response parse failure: {error}"))
                    })?;
                    return validate_vectors("TEI", vectors, inputs.len(), self.dimensions);
                }
                Ok((status, _)) if is_transient_status(status) && attempt < self.max_retries => {
                    std::thread::sleep(Duration::from_millis(50 * attempt as u64));
                }
                Ok((status, body)) if is_transient_status(status) => {
                    return Err(EmbeddingError::RetryExhausted {
                        provider: self.provider_name().to_string(),
                        attempts: attempt,
                        message: format!("TEI request failed with status {status}: {body}"),
                    });
                }
                Ok((status, body)) => {
                    return Err(EmbeddingError::RequestError(format!(
                        "TEI request failed with status {status}: {body}"
                    )));
                }
                Err(error)
                    if (error.is_timeout() || error.is_connect()) && attempt < self.max_retries =>
                {
                    std::thread::sleep(Duration::from_millis(50 * attempt as u64));
                }
                Err(error) if error.is_timeout() => {
                    return Err(EmbeddingError::Timeout {
                        provider: self.provider_name().to_string(),
                        message: error.to_string(),
                    });
                }
                Err(error) => {
                    return Err(EmbeddingError::RequestError(error.to_string()));
                }
            }
        }
    }

    fn run_blocking<T, F>(&self, f: F) -> Result<reqwest::Result<T>, EmbeddingError>
    where
        F: FnOnce() -> reqwest::Result<T>,
    {
        Ok(f())
    }
}

impl EmbeddingProvider for TeiProvider {
    fn provider_name(&self) -> &'static str {
        "tei"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        let mut out = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(self.max_batch_size) {
            out.extend(self.embed_documents_batch(chunk)?);
        }
        Ok(out)
    }
}

fn is_transient_status(status: reqwest::StatusCode) -> bool {
    status == 429 || status.is_server_error()
}

fn validate_vectors(
    provider: &str,
    vectors: Vec<Vec<f32>>,
    expected_count: usize,
    expected_dimensions: usize,
) -> Result<Vec<Embedding>, EmbeddingError> {
    if vectors.len() != expected_count {
        return Err(EmbeddingError::ParseError(format!(
            "{provider} response length {} does not match request length {expected_count}",
            vectors.len()
        )));
    }
    for vector in &vectors {
        if vector.len() != expected_dimensions {
            return Err(EmbeddingError::ParseError(format!(
                "{provider} embedding dimension {} does not match expected {expected_dimensions}",
                vector.len()
            )));
        }
    }
    Ok(vectors
        .into_iter()
        .map(|values| Embedding { values })
        .collect())
}
