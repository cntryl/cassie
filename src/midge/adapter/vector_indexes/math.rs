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

/// Per-list `[mean..., radius]` over the members at their original magnitude:
/// the mean vector and the largest member distance from it (all zero for an
/// empty list). Inner-product probing needs both because the lists themselves
/// are directional cells.
pub(super) fn list_bounds(
    records: &[crate::embeddings::NormalizedVectorRecord],
    assignments: &std::collections::BTreeMap<String, usize>,
    centroids: &[Vec<f32>],
) -> Vec<Vec<f32>> {
    let dimensions = centroids.first().map_or(0, Vec::len);
    let member = |record: &crate::embeddings::NormalizedVectorRecord| -> Option<(usize, Vec<f64>)> {
        let list = assignments.get(&record.id).copied()?;
        let raw = record
            .values
            .iter()
            .map(|value| f64::from(*value) * record.magnitude)
            .collect();
        (list < centroids.len()).then_some((list, raw))
    };
    let mut means = vec![vec![0.0_f64; dimensions]; centroids.len()];
    let mut counts = vec![0_u32; centroids.len()];
    for (list, raw) in records.iter().filter_map(member) {
        counts[list] = counts[list].saturating_add(1);
        for (total, value) in means[list].iter_mut().zip(&raw) {
            *total += value;
        }
    }
    for (mean, count) in means.iter_mut().zip(&counts) {
        for total in mean.iter_mut() {
            *total /= f64::from((*count).max(1));
        }
    }
    let mut radii = vec![0.0_f64; centroids.len()];
    for (list, raw) in records.iter().filter_map(member) {
        let distance = raw
            .iter()
            .zip(&means[list])
            .map(|(value, mean)| (value - mean).powi(2))
            .sum::<f64>()
            .sqrt();
        radii[list] = radii[list].max(distance);
    }
    means
        .into_iter()
        .zip(radii)
        .map(|(mean, radius)| {
            // The outward margin keeps the stored f32 radius from understating the list.
            mean.into_iter()
                .chain(std::iter::once(radius * (1.0 + 1e-5)))
                .map(|value| crate::vector::f64_to_finite_f32(value).unwrap_or(0.0))
                .collect()
        })
        .collect()
}
