pub mod cohere;
pub mod compatible;
pub mod error;
pub mod local;
pub mod ollama;
pub mod openai;
pub mod provider;
mod response;
pub mod tei;
pub mod types;
pub mod voyage;

pub use cassie_core::request::Interruption;
pub use error::EmbeddingError;
pub use openai::OpenAiConfig;
pub use provider::EmbeddingProvider;
pub use types::Embedding;

pub const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
