use crate::embeddings::provider::{
    controlled_backoff, controlled_request_timeout, run_controlled_request,
};
use crate::embeddings::response::{read_response, ResponseReadError};
use crate::embeddings::EmbeddingProvider;
use crate::runtime::QueryExecutionControls;
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::embeddings::{Embedding, EmbeddingError};

pub const DEFAULT_OPENAI_TIMEOUT_SECONDS: u64 = 30;
pub const DEFAULT_OPENAI_MAX_RETRIES: usize = 3;
pub const DEFAULT_OPENAI_MAX_BATCH_SIZE: usize = 16;

#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct OpenAiProviderConfig {
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
    pub max_batch_size: usize,
    pub max_retries: usize,
    pub base_url: String,
}

#[derive(Debug, Serialize, Clone)]
struct EmbeddingRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingData>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiEmbeddingData {
    pub index: usize,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Debug)]
pub struct OpenAiProvider {
    api_key: String,
    model: String,
    dimensions: usize,
    client: Client,
    request_timeout: Duration,
    base_url: String,
    max_batch_size: usize,
    max_retries: usize,
    max_response_bytes: usize,
}

impl OpenAiProvider {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn from_config(config: OpenAiConfig) -> Result<Self, EmbeddingError> {
        Self::with_config(OpenAiProviderConfig {
            api_key: config.api_key,
            model: config.model,
            timeout: Duration::from_secs(DEFAULT_OPENAI_TIMEOUT_SECONDS),
            max_batch_size: DEFAULT_OPENAI_MAX_BATCH_SIZE,
            max_retries: DEFAULT_OPENAI_MAX_RETRIES,
            base_url: "https://api.openai.com".to_string(),
        })
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn with_config(config: OpenAiProviderConfig) -> Result<Self, EmbeddingError> {
        if config.api_key.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "OpenAI API key is required".to_string(),
            ));
        }

        if config.model.trim().is_empty() {
            return Err(EmbeddingError::InvalidConfiguration(
                "OpenAI model is required".to_string(),
            ));
        }

        let dimensions = dimensions_for_model(&config.model)?;
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|error| {
                EmbeddingError::InvalidConfiguration(format!(
                    "failed to configure OpenAI client: {error}"
                ))
            })?;

        Ok(Self {
            api_key: config.api_key,
            model: config.model,
            dimensions,
            client,
            request_timeout: config.timeout,
            base_url: config.base_url,
            max_batch_size: config.max_batch_size.max(1),
            max_retries: config.max_retries.max(1),
            max_response_bytes: crate::embeddings::DEFAULT_MAX_RESPONSE_BYTES,
        })
    }

    pub(crate) fn with_max_response_bytes(mut self, max_response_bytes: usize) -> Self {
        self.max_response_bytes = max_response_bytes.max(1);
        self
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn test_with_base_url(
        config: OpenAiConfig,
        base_url: String,
    ) -> Result<Self, EmbeddingError> {
        Self::with_config(OpenAiProviderConfig {
            api_key: config.api_key,
            model: config.model,
            timeout: Duration::from_secs(1),
            max_batch_size: DEFAULT_OPENAI_MAX_BATCH_SIZE,
            max_retries: DEFAULT_OPENAI_MAX_RETRIES,
            base_url,
        })
    }

    fn is_transient_status(status: reqwest::StatusCode) -> bool {
        status == 429 || status.is_server_error()
    }

    fn request_embedding_batch(
        &self,
        endpoint: &str,
        request: EmbeddingRequest,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Result<(reqwest::StatusCode, String), ResponseReadError>, EmbeddingError> {
        let timeout =
            controlled_request_timeout(self.provider_name(), self.request_timeout, controls)?;
        let endpoint = endpoint.to_string();
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let max_response_bytes = self.max_response_bytes;
        run_controlled_request(self.provider_name(), controls, move || {
            let response = client
                .post(&endpoint)
                .timeout(timeout)
                .header("Authorization", format!("Bearer {api_key}"))
                .json(&request)
                .send()?;
            read_response(response, max_response_bytes)
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

        let request = EmbeddingRequest {
            model: self.model.clone(),
            input: inputs.to_vec(),
        };
        let endpoint = format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'));

        let mut attempt = 0usize;
        loop {
            attempt += 1;
            let started = Instant::now();
            let response = self.request_embedding_batch(&endpoint, request.clone(), controls)?;

            match response {
                Ok((status, response_body)) => {
                    let elapsed_ms =
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    if status.is_success() {
                        return self.parse_successful_embeddings(
                            &response_body,
                            elapsed_ms,
                            inputs.len(),
                        );
                    }

                    if Self::is_transient_status(status) && attempt < self.max_retries {
                        tracing::warn!(
                            provider = %self.provider_name(),
                            model = %self.model,
                            status = %status,
                            attempt,
                            max_retries = self.max_retries,
                            "transient OpenAI response; retrying"
                        );
                        let delay = Duration::from_millis(50 * attempt as u64);
                        controlled_backoff(self.provider_name(), delay, controls)?;
                        continue;
                    }

                    let status_message = format!("{status}");
                    return Err(EmbeddingError::RequestError(format!(
                        "openai request failed with status: {status_message}"
                    )));
                }
                Err(error @ ResponseReadError::TooLarge { .. }) => {
                    return Err(error.into_embedding_error(self.provider_name()));
                }
                Err(error) => {
                    let is_timeout = error.is_timeout();
                    if (is_timeout || error.is_connect()) && attempt < self.max_retries {
                        tracing::warn!(
                            provider = %self.provider_name(),
                            model = %self.model,
                            attempt,
                            max_retries = self.max_retries,
                            "transient OpenAI network error; retrying"
                        );
                        controlled_backoff(
                            self.provider_name(),
                            Duration::from_millis(50 * attempt as u64),
                            controls,
                        )?;
                        continue;
                    }

                    if is_timeout {
                        return Err(EmbeddingError::Timeout {
                            provider: self.provider_name().to_string(),
                            message: error.to_string(),
                        });
                    }

                    if attempt >= self.max_retries {
                        return Err(EmbeddingError::RetryExhausted {
                            provider: self.provider_name().to_string(),
                            attempts: attempt,
                            message: error.to_string(),
                        });
                    }

                    tracing::warn!(
                        provider = %self.provider_name(),
                        model = %self.model,
                        attempt,
                        "retrying OpenAI request"
                    );
                    controlled_backoff(
                        self.provider_name(),
                        Duration::from_millis(50 * attempt as u64),
                        controls,
                    )?;
                }
            }
        }
    }

    fn parse_successful_embeddings(
        &self,
        response_body: &str,
        elapsed_ms: u64,
        input_count: usize,
    ) -> Result<Vec<Embedding>, EmbeddingError> {
        let parsed: OpenAiEmbeddingResponse =
            serde_json::from_str(response_body).map_err(|error| {
                EmbeddingError::ParseError(format!("OpenAI response parse failure: {error}"))
            })?;
        self.log_success(elapsed_ms, input_count, parsed.usage.as_ref());
        let mut ordered = parsed.data;
        ordered.sort_by_key(|entry| entry.index);
        if ordered.len() != input_count {
            return Err(EmbeddingError::ParseError(
                "OpenAI response length does not match request length".to_string(),
            ));
        }
        for item in &ordered {
            if item.embedding.len() != self.dimensions {
                return Err(EmbeddingError::ParseError(format!(
                    "unexpected embedding dimension {} (expected {})",
                    item.embedding.len(),
                    self.dimensions
                )));
            }
        }
        Ok(ordered
            .into_iter()
            .map(|entry| Embedding {
                values: entry.embedding,
            })
            .collect())
    }

    fn log_success(&self, elapsed_ms: u64, input_count: usize, usage: Option<&OpenAiUsage>) {
        if let Some(usage) = usage {
            tracing::info!(
                provider = %self.provider_name(),
                model = %self.model,
                latency_ms = elapsed_ms,
                prompt_tokens = usage.prompt_tokens.unwrap_or(0),
                total_tokens = usage.total_tokens.unwrap_or(0),
                dimensions = self.dimensions,
                "embeddings request completed"
            );
        } else {
            tracing::info!(
                provider = %self.provider_name(),
                model = %self.model,
                latency_ms = elapsed_ms,
                batch = input_count,
                dimensions = self.dimensions,
                "embeddings request completed"
            );
        }
    }
}

impl EmbeddingProvider for OpenAiProvider {
    fn provider_name(&self) -> &'static str {
        "openai"
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let mut out = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(self.max_batch_size) {
            let chunk = self.embed_documents_batch(chunk, None)?;
            out.extend(chunk);
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

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
pub fn dimensions_for_model(model: &str) -> Result<usize, EmbeddingError> {
    match model {
        "text-embedding-3-large" => Ok(3072),
        "text-embedding-3-small" | "text-embedding-ada-002" => Ok(1536),
        model => Err(EmbeddingError::InvalidConfiguration(format!(
            "unsupported OpenAI model: {model}"
        ))),
    }
}
