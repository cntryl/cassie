pub fn score(query: &[f32], target: &[f32]) -> f64 {
    if query.len() != target.len() || query.is_empty() {
        return 0.0;
    }
    let mut dot = 0f64;
    for (q, t) in query.iter().zip(target.iter()) {
        dot += *q as f64 * *t as f64;
    }
    dot
}

pub fn distance(query: &[f32], target: &[f32]) -> f64 {
    -score(query, target)
}
