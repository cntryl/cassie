pub fn distance(query: &[f32], target: &[f32]) -> f64 {
    if query.len() != target.len() {
        return f64::MAX;
    }
    super::simd::squared_l2(query, target).sqrt()
}

pub fn score(query: &[f32], target: &[f32]) -> f64 {
    let d = distance(query, target);
    if d.is_infinite() {
        0.0
    } else {
        1.0 / (1.0 + d)
    }
}
