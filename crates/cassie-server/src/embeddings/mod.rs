pub mod cohere;
pub mod compatible;
pub mod error;
pub mod local;
pub mod ollama;
pub mod openai;
pub mod provider;
pub(crate) mod response;
pub mod tei;
pub mod types;
pub mod voyage;

pub use crate::embeddings::error::EmbeddingError;
pub use crate::embeddings::openai::OpenAiConfig;
pub use crate::embeddings::provider::EmbeddingProvider;
pub use crate::embeddings::types::{
    DistanceMetric, Embedding, HnswGraphNode, HnswGraphState, HnswIndexOptions,
    IvfFlatIndexOptions, IvfFlatTrainingState, NormalizedVectorRecord, VectorIndexMetadata,
    VectorIndexRecord, VectorIndexState, VectorIndexType,
};

pub const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";
pub(crate) const DEFAULT_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
