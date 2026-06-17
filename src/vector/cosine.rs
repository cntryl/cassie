pub fn score(query: &[f32], target: &[f32]) -> f64 {
    if query.is_empty() || target.is_empty() || query.len() != target.len() {
        return 0.0;
    }
    let mut dot = 0f64;
    let mut qnorm = 0f64;
    let mut nnorm = 0f64;
    for (q, t) in query.iter().zip(target.iter()) {
        let qv = *q as f64;
        let tv = *t as f64;
        dot += qv * tv;
        qnorm += qv * qv;
        nnorm += tv * tv;
    }
    if qnorm == 0.0 || nnorm == 0.0 {
        return 0.0;
    }
    dot / (qnorm.sqrt() * nnorm.sqrt())
}

pub fn distance(query: &[f32], target: &[f32]) -> f64 {
    1.0 - score(query, target)
}
