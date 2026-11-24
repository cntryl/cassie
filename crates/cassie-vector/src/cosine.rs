#[must_use]
pub fn score(query: &[f32], target: &[f32]) -> f64 {
    if query.is_empty() || target.is_empty() || query.len() != target.len() {
        return 0.0;
    }

    let (dot, qnorm, nnorm) = super::simd::cosine_components(query, target);
    if qnorm == 0.0 || nnorm == 0.0 {
        return 0.0;
    }
    dot / (qnorm.sqrt() * nnorm.sqrt())
}

#[must_use]
pub fn distance(query: &[f32], target: &[f32]) -> f64 {
    1.0 - score(query, target)
}
