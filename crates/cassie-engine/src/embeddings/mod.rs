pub use cassie_embeddings::{
    cohere, compatible, error, local, ollama, openai, provider, tei, voyage, EmbeddingError,
    EmbeddingProvider, Interruption, OpenAiConfig, DEFAULT_EMBEDDING_MODEL,
};

pub mod types;

pub use types::{
    DistanceMetric, Embedding, HnswGraphNode, HnswGraphState, HnswIndexOptions,
    IvfFlatIndexOptions, IvfFlatTrainingState, NormalizedVectorRecord, VectorIndexMetadata,
    VectorIndexRecord, VectorIndexState, VectorIndexType,
};

pub const DEFAULT_MAX_RESPONSE_BYTES: usize = cassie_embeddings::DEFAULT_MAX_RESPONSE_BYTES;
