use crate::embeddings::EmbeddingProvider;
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};

use crate::embeddings::{Embedding, EmbeddingError};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleProviderConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub dimensions: usize,
    pub timeout: Duration,
    pub max_batch_size: usize,
    pub max_retries: usize,
}

#[derive(Debug)]
pub struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: Option<String>,
    model: String,
    dimensions: usize,
    client: Client,
    max_batch_size: usize,
    max_retries: usize,
}

#[derive(Debug, Serialize, Clone)]
struct OpenAiCompatibleRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleResponse {
    data: Vec<OpenAiCompatibleEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompatibleEmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

impl OpenAiCompatibleProvider {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn with_config(config: OpenAiCompatibleProviderConfig) -> Result<Self, EmbeddingError> {
        if config.base_url.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "OpenAI-compatible base URL is required".to_string(),
            ));
        }
        if config.model.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "OpenAI-compatible model is required".to_string(),
            ));
        }
        if config.dimensions == 0 {
            return Err(EmbeddingError::InvalidConfiguration(
                "OpenAI-compatible dimensions must be greater than zero".to_string(),
            ));
        }

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|error| {
                EmbeddingError::InvalidConfiguration(format!(
                    "failed to configure OpenAI-compatible client: {error}"
                ))
            })?;

        Ok(Self {
            base_url: config.base_url,
            api_key: config.api_key.filter(|value| !value.is_empty()),
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

        let request = OpenAiCompatibleRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };
        let endpoint = format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'));

        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let client = self.client.clone();
            let endpoint = endpoint.clone();
            let request_snapshot = request.clone();
            let api_key = self.api_key.clone();
            let response = self.run_blocking(move || {
                let builder = client.post(endpoint).json(&request_snapshot);
                let builder = add_auth_header(builder, api_key.as_deref());
                let response = builder.send()?;
                let status = response.status();
                let body = response.text()?;
                Ok::<_, reqwest::Error>((status, body))
            })?;

            match response {
                Ok((status, body)) if status.is_success() => {
                    let parsed: OpenAiCompatibleResponse =
                        serde_json::from_str(&body).map_err(|error| {
                            EmbeddingError::ParseError(format!(
                                "OpenAI-compatible response parse failure: {error}"
                            ))
                        })?;
                    return validate_embeddings(parsed.data, inputs.len(), self.dimensions);
                }
                Ok((status, _)) if is_transient_status(status) && attempt < self.max_retries => {
                    std::thread::sleep(Duration::from_millis(50 * attempt as u64));
                }
                Ok((status, body)) if is_transient_status(status) => {
                    return Err(EmbeddingError::RetryExhausted {
                        provider: self.provider_name().to_string(),
                        attempts: attempt,
                        message: format!(
                            "OpenAI-compatible request failed with status {status}: {body}"
                        ),
                    });
                }
                Ok((status, body)) => {
                    return Err(EmbeddingError::RequestError(format!(
                        "OpenAI-compatible request failed with status {status}: {body}"
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

impl EmbeddingProvider for OpenAiCompatibleProvider {
    fn provider_name(&self) -> &'static str {
        "openai_compatible"
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

fn add_auth_header(builder: RequestBuilder, api_key: Option<&str>) -> RequestBuilder {
    match api_key {
        Some(api_key) => builder.header("Authorization", format!("Bearer {api_key}")),
        None => builder,
    }
}

fn is_transient_status(status: reqwest::StatusCode) -> bool {
    status == 429 || status.is_server_error()
}

fn validate_embeddings(
    mut data: Vec<OpenAiCompatibleEmbeddingData>,
    expected_count: usize,
    expected_dimensions: usize,
) -> Result<Vec<Embedding>, EmbeddingError> {
    data.sort_by_key(|entry| entry.index);
    if data.len() != expected_count {
        return Err(EmbeddingError::ParseError(format!(
            "OpenAI-compatible response length {} does not match request length {expected_count}",
            data.len()
        )));
    }
    for item in &data {
        if item.embedding.len() != expected_dimensions {
            return Err(EmbeddingError::ParseError(format!(
                "OpenAI-compatible embedding dimension {} does not match expected {expected_dimensions}",
                item.embedding.len()
            )));
        }
    }
    Ok(data
        .into_iter()
        .map(|entry| Embedding {
            values: entry.embedding,
        })
        .collect())
}
