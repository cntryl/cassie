use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::embeddings::EmbeddingProvider;
use crate::embeddings::{Embedding, EmbeddingError};

#[derive(Debug, Clone)]
pub struct CohereProviderConfig {
    pub api_key: String,
    pub model: String,
    pub dimensions: usize,
    pub timeout: Duration,
    pub max_batch_size: usize,
    pub max_retries: usize,
    pub base_url: String,
}

#[derive(Debug)]
pub struct CohereProvider {
    api_key: String,
    model: String,
    dimensions: usize,
    client: Client,
    max_batch_size: usize,
    max_retries: usize,
    base_url: String,
}

#[derive(Debug, Serialize, Clone)]
struct CohereEmbedRequest {
    model: String,
    input_type: &'static str,
    texts: Vec<String>,
    embedding_types: Vec<&'static str>,
    output_dimension: usize,
}

#[derive(Debug, Deserialize)]
struct CohereEmbedResponse {
    embeddings: CohereEmbeddings,
}

#[derive(Debug, Deserialize)]
struct CohereEmbeddings {
    float: Vec<Vec<f32>>,
}

impl CohereProvider {
    /// # Errors
    ///
    /// Returns an error when the provider configuration is invalid.
    pub fn with_config(config: CohereProviderConfig) -> Result<Self, EmbeddingError> {
        if config.api_key.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "Cohere API key is required".to_string(),
            ));
        }
        if config.model.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "Cohere model is required".to_string(),
            ));
        }
        if config.dimensions == 0 {
            return Err(EmbeddingError::InvalidConfiguration(
                "Cohere dimensions must be greater than zero".to_string(),
            ));
        }
        if config.base_url.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "Cohere base URL is required".to_string(),
            ));
        }

        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|error| {
                EmbeddingError::InvalidConfiguration(format!(
                    "failed to configure Cohere client: {error}"
                ))
            })?;

        Ok(Self {
            api_key: config.api_key,
            model: config.model,
            dimensions: config.dimensions,
            client,
            max_batch_size: config.max_batch_size.max(1),
            max_retries: config.max_retries.max(1),
            base_url: config.base_url,
        })
    }

    fn embed_batch(
        &self,
        inputs: &[String],
        input_type: &'static str,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let request = CohereEmbedRequest {
            model: self.model.clone(),
            input_type,
            texts: inputs.to_vec(),
            embedding_types: vec!["float"],
            output_dimension: self.dimensions,
        };
        let endpoint = format!("{}/v2/embed", self.base_url.trim_end_matches('/'));

        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let client = self.client.clone();
            let endpoint = endpoint.clone();
            let request_snapshot = request.clone();
            let api_key = self.api_key.clone();

            let response = Self::run_blocking(move || {
                let response = client
                    .post(endpoint)
                    .header("Authorization", format!("Bearer {api_key}"))
                    .json(&request_snapshot)
                    .send()?;
                let status = response.status();
                let body = response.text()?;
                Ok::<_, reqwest::Error>((status, body))
            });

            match response {
                Ok((status, body)) if status.is_success() => {
                    let parsed: CohereEmbedResponse =
                        serde_json::from_str(&body).map_err(|error| {
                            EmbeddingError::ParseError(format!(
                                "Cohere response parse failure: {error}"
                            ))
                        })?;
                    return validate_embeddings(
                        parsed.embeddings.float,
                        inputs.len(),
                        self.dimensions,
                    );
                }
                Ok((status, body)) if is_transient_status(status) && attempt < self.max_retries => {
                    std::thread::sleep(Duration::from_millis(50 * attempt as u64));
                    tracing::warn!(
                        provider = %self.provider_name(),
                        model = %self.model,
                        status = %status,
                        attempt,
                        "retrying Cohere embedding request"
                    );
                    let _ = body;
                }
                Ok((status, body)) if is_transient_status(status) => {
                    return Err(EmbeddingError::RetryExhausted {
                        provider: self.provider_name().to_string(),
                        attempts: attempt,
                        message: format!("Cohere request failed with status {status}: {body}"),
                    });
                }
                Ok((status, body)) => {
                    return Err(EmbeddingError::RequestError(format!(
                        "Cohere request failed with status {status}: {body}"
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

    fn run_blocking<T, F>(f: F) -> reqwest::Result<T>
    where
        F: FnOnce() -> reqwest::Result<T>,
    {
        f()
    }
}

impl EmbeddingProvider for CohereProvider {
    fn provider_name(&self) -> &'static str {
        "cohere"
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
            out.extend(self.embed_batch(chunk, "search_document")?);
        }
        Ok(out)
    }

    fn embed_query(&self, input: &str) -> Result<Embedding, EmbeddingError> {
        self.embed_batch(std::slice::from_ref(&input.to_string()), "search_query")
            .map(|mut embeddings| embeddings.remove(0))
    }
}

fn is_transient_status(status: reqwest::StatusCode) -> bool {
    status == 429 || status.is_server_error()
}

fn validate_embeddings(
    embeddings: Vec<Vec<f32>>,
    expected_count: usize,
    expected_dimensions: usize,
) -> Result<Vec<Embedding>, EmbeddingError> {
    if embeddings.len() != expected_count {
        return Err(EmbeddingError::ParseError(format!(
            "Cohere response length {} does not match request length {expected_count}",
            embeddings.len()
        )));
    }

    for embedding in &embeddings {
        if embedding.len() != expected_dimensions {
            return Err(EmbeddingError::ParseError(format!(
                "Cohere embedding dimension {} does not match expected {expected_dimensions}",
                embedding.len()
            )));
        }
    }

    Ok(embeddings
        .into_iter()
        .map(|values| Embedding { values })
        .collect())
}
