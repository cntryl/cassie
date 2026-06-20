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
    scored.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
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

#[cfg(test)]
mod tests {
    use super::top_k;

    #[test]
    fn should_return_nearest_candidates_by_distance() {
        // Arrange
        let query = vec![1.0, 0.0];
        let candidates = vec![
            ("far".to_string(), vec![5.0, 0.0]),
            ("nearest".to_string(), vec![1.0, 0.0]),
            ("middle".to_string(), vec![2.0, 0.0]),
        ];

        // Act
        let selected = top_k(query, candidates, 2, crate::vector::l2_distance);

        // Assert
        assert_eq!(
            selected,
            vec![("nearest".to_string(), 0.0), ("middle".to_string(), 1.0)]
        );
    }
}
