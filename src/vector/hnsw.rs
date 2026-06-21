#[derive(Debug, Clone, PartialEq)]
pub struct HnswCandidate {
    pub id: String,
    pub distance: f64,
}

pub fn search(
    query: &[f32],
    candidates: impl IntoIterator<Item = (String, Vec<f32>)>,
    limit: usize,
    metric: fn(&[f32], &[f32]) -> f64,
) -> Vec<HnswCandidate> {
    let mut scored = candidates
        .into_iter()
        .filter(|(_, vector)| vector.len() == query.len())
        .map(|(id, vector)| HnswCandidate {
            distance: metric(query, &vector),
            id,
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| {
        left.distance
            .total_cmp(&right.distance)
            .then_with(|| left.id.cmp(&right.id))
    });
    scored.truncate(limit.max(1));
    scored
}

#[cfg(test)]
mod tests {
    use super::search;

    #[test]
    fn should_return_exactly_ranked_hnsw_candidates() {
        // Arrange
        let query = vec![0.0, 0.0];
        let candidates = vec![
            ("far".to_string(), vec![2.0, 0.0]),
            ("near".to_string(), vec![1.0, 0.0]),
            ("wrong-dim".to_string(), vec![0.0]),
        ];

        // Act
        let selected = search(&query, candidates, 2, crate::vector::l2_distance);

        // Assert
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].id, "near");
        assert_eq!(selected[1].id, "far");
    }
}
