pub mod brute_force;
pub mod cosine;
pub mod dot;
pub mod hnsw;
pub mod index_options;
pub mod ivfflat;
pub mod l2;
pub mod normalized;
mod simd;
pub mod source_fingerprint;

pub use brute_force::top_k;
pub use cosine::{distance as cosine_distance, score as cosine_score};
pub use dot::{distance as dot_distance, score as dot_score};
pub use l2::{distance as l2_distance, score as l2_score};
pub use normalized::{
    cosine_distance_from_normalized_query, dot_distance_from_normalized_target, normalize,
    NormalizedVector,
};
pub use source_fingerprint::normalized_vector_source_fingerprint;
