use crate::types::Value;

pub fn top_k(
    query: Vec<f32>,
    candidates: Vec<(String, Vec<f32>)>,
    k: usize,
    metric: fn(&[f32], &[f32]) -> f64,
) -> Vec<(String, f64)> {
    let mut scored = Vec::with_capacity(candidates.len());
    for (id, vector) in candidates {
        let score = metric(&query, &vector);
        scored.push((id, score));
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    scored
}

#[allow(dead_code)]
pub fn extract_vectors(rows: &Vec<(String, Value)>) -> Vec<(String, Vec<f32>)> {
    let mut out = Vec::new();
    for (id, val) in rows {
        if let Value::Vector(v) = val {
            out.push((id.clone(), v.values.clone()));
        }
    }
    out
}
