pub fn distance(query: &[f32], target: &[f32]) -> f64 {
    if query.len() != target.len() {
        return f64::MAX;
    }
    let mut sum = 0f64;
    for (q, t) in query.iter().zip(target.iter()) {
        let d = *q as f64 - *t as f64;
        sum += d * d;
    }
    sum.sqrt()
}

pub fn score(query: &[f32], target: &[f32]) -> f64 {
    let d = distance(query, target);
    if d.is_infinite() {
        0.0
    } else {
        1.0 / (1.0 + d)
    }
}
