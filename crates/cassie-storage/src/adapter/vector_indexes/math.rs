pub(super) fn ivfflat_training_order(seed: u64, id: &str) -> u64 {
    let mut state = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for byte in id.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x0100_0000_01b3);
    }
    state
}

pub(super) fn nearest_ivfflat_centroid(vector: &[f32], centroids: &[Vec<f32>]) -> usize {
    centroids
        .iter()
        .enumerate()
        .min_by(|(left_index, left), (right_index, right)| {
            squared_l2(vector, left)
                .total_cmp(&squared_l2(vector, right))
                .then_with(|| left_index.cmp(right_index))
        })
        .map_or(0, |(index, _)| index)
}

fn squared_l2(left: &[f32], right: &[f32]) -> f64 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| {
            let delta = f64::from(*left) - f64::from(*right);
            delta * delta
        })
        .sum()
}

/// Per-list `[center..., radius]` at the members' original magnitude. A
/// missing set tells inner-product probing to score exactly.
pub(super) fn list_bounds(
    records: &[crate::embeddings::NormalizedVectorRecord],
    assignments: &std::collections::BTreeMap<String, usize>,
    centroids: &[Vec<f32>],
) -> Vec<Vec<f32>> {
    conservative_list_bounds(records, assignments, centroids).unwrap_or_default()
}

fn conservative_list_bounds(
    records: &[crate::embeddings::NormalizedVectorRecord],
    assignments: &std::collections::BTreeMap<String, usize>,
    centroids: &[Vec<f32>],
) -> Option<Vec<Vec<f32>>> {
    let dimensions = centroids.first()?.len();
    let mut means = vec![vec![0.0_f64; dimensions]; centroids.len()];
    let mut counts = vec![0_u32; centroids.len()];
    for record in records {
        let list = *assignments.get(&record.id)?;
        let mean = means.get_mut(list)?;
        if record.values.len() != dimensions
            || !record.magnitude.is_finite()
            || record.magnitude < 0.0
        {
            return None;
        }
        counts[list] = counts[list].saturating_add(1);
        for (total, normalized) in mean.iter_mut().zip(&record.values) {
            *total += f64::from(*normalized) * record.magnitude;
        }
    }

    // Measure distance from the rounded center that readers actually use.
    let mut bounds = means
        .into_iter()
        .zip(counts)
        .map(|(sum, count)| {
            sum.into_iter()
                .map(|value| crate::vector::f64_to_finite_f32(value / f64::from(count.max(1))))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()?;
    let mut radii = vec![0.0_f64; bounds.len()];
    for record in records {
        let list = assignments[&record.id];
        let distance = record
            .values
            .iter()
            .zip(&bounds[list])
            .map(|(value, center)| {
                (f64::from(*value) * record.magnitude - f64::from(*center)).powi(2)
            })
            .sum::<f64>()
            .sqrt();
        // f32 normalization can move the reconstructed vector by about one
        // f32 ulp of its magnitude. Reserve a larger margin for that error,
        // including subnormals, and f64 accumulation in the supported dimensions.
        let conservative_distance = distance + (distance + record.magnitude) * 1e-6;
        radii[list] = radii[list].max(conservative_distance);
    }

    for (bound, radius) in bounds.iter_mut().zip(radii) {
        let rounded = crate::vector::f64_to_finite_f32(radius)?;
        bound.push(if f64::from(rounded) < radius {
            rounded.next_up()
        } else {
            rounded
        });
    }
    Some(bounds)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::embeddings::{DistanceMetric, NormalizedVectorRecord};

    use super::list_bounds;

    #[test]
    fn should_bound_original_coordinates_after_rounding_normalized_vectors() {
        // Arrange
        let rounded = [0.1_f32, 1.0e-35];
        let ordinary = [[-2.0_f32, 3.0], [4.0, -6.5]];
        let vectors = [rounded, ordinary[0], ordinary[1]];
        let centroids = vec![vec![1.0, 0.0], vec![0.0, 1.0]];
        let records = vectors.map(|vector| crate::vector::normalize(&vector).expect("normalize"));
        let mut assignments = BTreeMap::new();
        let records = records
            .into_iter()
            .enumerate()
            .map(|(position, record)| {
                let id = position.to_string();
                assignments.insert(id.clone(), usize::from(position != 0));
                NormalizedVectorRecord {
                    collection: "vectors".to_owned(),
                    field: "embedding".to_owned(),
                    id,
                    built_generation: 0,
                    dimensions: 2,
                    metric: DistanceMetric::Dot,
                    normalization_version: 1,
                    values: record.values,
                    magnitude: record.magnitude,
                    payload_available: true,
                }
            })
            .collect::<Vec<_>>();

        // Act
        let bounds = list_bounds(&records, &assignments, &centroids);

        // Assert
        for (record, vector) in records.iter().zip(vectors) {
            let bound = &bounds[assignments[&record.id]];
            let (center, radius) = bound.split_at(2);
            for query in [[0.0, -1.0], [1.0, 0.0], [0.0, 1.0], [-5.0, 7.0]] {
                let actual = crate::vector::dot_score(&query, &vector);
                let reach = crate::vector::dot_score(&query, center)
                    + crate::vector::dot_score(&query, &query).sqrt() * f64::from(radius[0]);
                assert!(
                    reach >= actual,
                    "query {query:?}, vector {vector:?}, bound {bound:?}"
                );
            }
        }
    }
}
