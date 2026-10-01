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

pub(crate) fn f64_to_finite_f32(value: f64) -> Option<f32> {
    if !value.is_finite() || value < f64::from(f32::MIN) || value > f64::from(f32::MAX) {
        return None;
    }
    value
        .to_string()
        .parse::<f32>()
        .ok()
        .filter(|component| component.is_finite())
}

pub(crate) fn finite_f32_vector(values: &[f32]) -> Option<Vec<f32>> {
    values
        .iter()
        .all(|component| component.is_finite())
        .then(|| values.to_vec())
}
