pub fn score(query: &[f32], target: &[f32]) -> f64 {
    if query.len() != target.len() || query.is_empty() {
        return 0.0;
    }
    super::simd::dot(query, target)
}

pub fn distance(query: &[f32], target: &[f32]) -> f64 {
    -score(query, target)
}
