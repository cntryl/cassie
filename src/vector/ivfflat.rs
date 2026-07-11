use std::collections::BTreeSet;

use crate::embeddings::{IvfFlatTrainingState, NormalizedVectorRecord};

#[must_use]
pub fn training_fallback_reason(
    training: &IvfFlatTrainingState,
    dimensions: usize,
    records: &[NormalizedVectorRecord],
) -> Option<&'static str> {
    if training.source_fingerprint == 0 {
        return Some("missing-source-fingerprint");
    }
    if training.source_fingerprint != crate::vector::normalized_vector_source_fingerprint(records) {
        return Some("stale-source-fingerprint");
    }
    if !training.trained {
        return Some("untrained");
    }
    if training.row_count != records.len() {
        return Some("stale-row-count");
    }
    if training.lists == 0 || training.centroids.len() != training.lists {
        return Some("invalid-centroid-count");
    }
    if training.list_sizes.len() != training.lists {
        return Some("invalid-list-sizes");
    }
    if training.probes == 0 || training.probes > training.lists {
        return Some("invalid-probes");
    }
    if training
        .centroids
        .iter()
        .any(|centroid| centroid.len() != dimensions)
    {
        return Some("incompatible-centroid-dimensions");
    }

    let current_ids = records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<BTreeSet<_>>();
    if training.assignments.len() != current_ids.len() {
        return Some("incomplete-assignments");
    }

    let mut observed_sizes = vec![0usize; training.lists];
    for id in current_ids {
        let Some(list) = training.assignments.get(id) else {
            return Some("missing-assignment");
        };
        if *list >= training.lists {
            return Some("assignment-list-out-of-bounds");
        }
        observed_sizes[*list] = observed_sizes[*list].saturating_add(1);
    }

    let assigned_ids = training
        .assignments
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if assigned_ids.len() != training.assignments.len() {
        return Some("duplicate-assignment-id");
    }
    if observed_sizes != training.list_sizes {
        return Some("stale-list-sizes");
    }
    None
}

#[must_use]
pub fn training_compatible(
    training: &IvfFlatTrainingState,
    dimensions: usize,
    records: &[NormalizedVectorRecord],
) -> bool {
    training_fallback_reason(training, dimensions, records).is_none()
}

#[must_use]
pub fn probe_lists(normalized_query: &[f32], training: &IvfFlatTrainingState) -> BTreeSet<usize> {
    let mut ranked = training
        .centroids
        .iter()
        .enumerate()
        .map(|(index, centroid)| (index, squared_l2(normalized_query, centroid)))
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked
        .into_iter()
        .take(training.probes.max(1))
        .map(|(index, _)| index)
        .collect()
}

#[must_use]
pub fn denormalized_vector(record: &NormalizedVectorRecord) -> Option<Vec<f32>> {
    let magnitude = record.magnitude.to_string().parse::<f32>().ok()?;
    Some(
        record
            .values
            .iter()
            .map(|value| *value * magnitude)
            .collect(),
    )
}

fn squared_l2(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() {
        return f64::INFINITY;
    }
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| {
            let delta = f64::from(*left) - f64::from(*right);
            delta * delta
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{denormalized_vector, probe_lists, training_fallback_reason};
    use crate::embeddings::{DistanceMetric, IvfFlatTrainingState, NormalizedVectorRecord};
    use std::collections::BTreeMap;

    #[test]
    fn should_probe_nearest_centroid_lists() {
        // Arrange
        let training = IvfFlatTrainingState {
            version: 1,
            source_fingerprint: 1,
            trained: true,
            row_count: 2,
            lists: 2,
            probes: 1,
            training_seed: 1,
            centroid_ids: vec!["a".to_string(), "b".to_string()],
            centroids: vec![vec![1.0, 0.0], vec![0.0, 1.0]],
            assignments: BTreeMap::new(),
            list_sizes: vec![1, 1],
        };

        // Act
        let probed = probe_lists(&[0.9, 0.1], &training);

        // Assert
        assert!(probed.contains(&0));
        assert!(!probed.contains(&1));
    }

    #[test]
    fn should_denormalize_vector_record() {
        // Arrange
        let record = NormalizedVectorRecord {
            built_generation: 0,
            collection: "docs".to_string(),
            field: "embedding".to_string(),
            id: "doc".to_string(),
            dimensions: 2,
            metric: DistanceMetric::L2,
            normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
            payload_available: true,
            magnitude: 5.0,
            values: vec![0.6, 0.8],
        };

        // Act
        let values = denormalized_vector(&record).expect("denormalized vector");

        // Assert
        assert_eq!(values, vec![3.0, 4.0]);
    }

    #[test]
    fn should_report_stale_ivfflat_source_fingerprint() {
        // Arrange
        let records = vec![NormalizedVectorRecord {
            built_generation: 0,
            collection: "docs".to_string(),
            field: "embedding".to_string(),
            id: "doc".to_string(),
            dimensions: 2,
            metric: DistanceMetric::L2,
            normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
            payload_available: true,
            magnitude: 1.0,
            values: vec![1.0, 0.0],
        }];
        let mut assignments = BTreeMap::new();
        assignments.insert("doc".to_string(), 0);
        let training = IvfFlatTrainingState {
            version: 1,
            source_fingerprint: 1,
            trained: true,
            row_count: 1,
            lists: 1,
            probes: 1,
            training_seed: 1,
            centroid_ids: vec!["doc".to_string()],
            centroids: vec![vec![1.0, 0.0]],
            assignments,
            list_sizes: vec![1],
        };

        // Act
        let reason = training_fallback_reason(&training, 2, &records);

        // Assert
        assert_eq!(reason, Some("stale-source-fingerprint"));
    }

    #[test]
    fn should_report_missing_ivfflat_assignment_coverage() {
        // Arrange
        let records = vec![NormalizedVectorRecord {
            built_generation: 0,
            collection: "docs".to_string(),
            field: "embedding".to_string(),
            id: "doc".to_string(),
            dimensions: 2,
            metric: DistanceMetric::L2,
            normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
            payload_available: true,
            magnitude: 1.0,
            values: vec![1.0, 0.0],
        }];
        let training = IvfFlatTrainingState {
            version: 1,
            source_fingerprint: crate::vector::normalized_vector_source_fingerprint(&records),
            trained: true,
            row_count: 1,
            lists: 1,
            probes: 1,
            training_seed: 1,
            centroid_ids: vec!["doc".to_string()],
            centroids: vec![vec![1.0, 0.0]],
            assignments: BTreeMap::new(),
            list_sizes: vec![1],
        };

        // Act
        let reason = training_fallback_reason(&training, 2, &records);

        // Assert
        assert_eq!(reason, Some("incomplete-assignments"));
    }
}
