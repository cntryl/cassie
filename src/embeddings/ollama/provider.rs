use crate::embeddings::provider::{
    controlled_backoff, controlled_request_timeout, run_controlled_request,
};
use crate::embeddings::EmbeddingProvider;
use crate::runtime::QueryExecutionControls;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::embeddings::{Embedding, EmbeddingError};

#[derive(Debug, Clone)]
pub struct OllamaProviderConfig {
    pub base_url: String,
    pub model: String,
    pub dimensions: usize,
    pub timeout: Duration,
    pub max_batch_size: usize,
    pub max_retries: usize,
}

#[derive(Debug)]
pub struct OllamaProvider {
    base_url: String,
    model: String,
    dimensions: usize,
    client: Client,
    request_timeout: Duration,
    max_batch_size: usize,
    max_retries: usize,
}

#[derive(Debug, Serialize, Clone)]
struct OllamaRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaProvider {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn with_config(config: OllamaProviderConfig) -> Result<Self, EmbeddingError> {
        if config.base_url.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "Ollama base URL is required".to_string(),
            ));
        }
        if config.model.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "Ollama model is required".to_string(),
            ));
        }
        if config.dimensions == 0 {
            return Err(EmbeddingError::InvalidConfiguration(
                "Ollama dimensions must be greater than zero".to_string(),
            ));
        }

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|error| {
                EmbeddingError::InvalidConfiguration(format!(
                    "failed to configure Ollama client: {error}"
                ))
            })?;

        Ok(Self {
            base_url: config.base_url,
            model: config.model,
            dimensions: config.dimensions,
            client,
            request_timeout: config.timeout,
            max_batch_size: config.max_batch_size.max(1),
            max_retries: config.max_retries.max(1),
        })
    }

    fn embed_documents_batch(
        &self,
        inputs: &[String],
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let request = OllamaRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };
        let endpoint = format!("{}/api/embed", self.base_url.trim_end_matches('/'));

        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let timeout =
                controlled_request_timeout(self.provider_name(), self.request_timeout, controls)?;
            let client = self.client.clone();
            let endpoint = endpoint.clone();
            let request_snapshot = request.clone();
            let response = run_controlled_request(self.provider_name(), controls, move || {
                let response = client
                    .post(endpoint)
                    .timeout(timeout)
                    .json(&request_snapshot)
                    .send()?;
                let status = response.status();
                let body = response.text()?;
                Ok::<_, reqwest::Error>((status, body))
            })?;

            match response {
                Ok((status, body)) if status.is_success() => {
                    let parsed: OllamaResponse = serde_json::from_str(&body).map_err(|error| {
                        EmbeddingError::ParseError(format!(
                            "Ollama response parse failure: {error}"
                        ))
                    })?;
                    return validate_vectors(
                        "Ollama",
                        parsed.embeddings,
                        inputs.len(),
                        self.dimensions,
                    );
                }
                Ok((status, _)) if is_transient_status(status) && attempt < self.max_retries => {
                    controlled_backoff(
                        self.provider_name(),
                        Duration::from_millis(50 * attempt as u64),
                        controls,
                    )?;
                }
                Ok((status, body)) if is_transient_status(status) => {
                    return Err(EmbeddingError::RetryExhausted {
                        provider: self.provider_name().to_string(),
                        attempts: attempt,
                        message: format!("Ollama request failed with status {status}: {body}"),
                    });
                }
                Ok((status, body)) => {
                    return Err(EmbeddingError::RequestError(format!(
                        "Ollama request failed with status {status}: {body}"
                    )));
                }
                Err(error)
                    if (error.is_timeout() || error.is_connect()) && attempt < self.max_retries =>
                {
                    controlled_backoff(
                        self.provider_name(),
                        Duration::from_millis(50 * attempt as u64),
                        controls,
                    )?;
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
}

impl EmbeddingProvider for OllamaProvider {
    fn provider_name(&self) -> &'static str {
        "ollama"
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
            out.extend(self.embed_documents_batch(chunk, None)?);
        }
        Ok(out)
    }

    fn embed_documents_with_controls(
        &self,
        inputs: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        let mut out = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(self.max_batch_size) {
            out.extend(self.embed_documents_batch(chunk, Some(controls))?);
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
