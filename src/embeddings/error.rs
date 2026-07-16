use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("embedding request to '{provider}' was canceled")]
    Cancelled { provider: String },

    #[error("embedding provider unavailable: {provider} ({reason})")]
    Unavailable { provider: String, reason: String },

    #[error("embedding provider '{provider}' does not support this operation")]
    NotImplemented { provider: String },

    #[error("invalid embedding configuration: {0}")]
    InvalidConfiguration(String),

    #[error("embedding request to '{provider}' timed out: {message}")]
    Timeout { provider: String, message: String },

    #[error("transient request failures exhausted for provider '{provider}' after {attempts} attempts: {message}")]
    RetryExhausted {
        provider: String,
        attempts: usize,
        message: String,
    },

    #[error("embedding parse error: {0}")]
    ParseError(String),

    #[error("embedding request error: {0}")]
    RequestError(String),
}
