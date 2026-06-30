use super::simd;

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedVector {
    pub values: Vec<f32>,
    pub magnitude: f64,
}

/// Normalizes each dimension and returns the resulting vector plus magnitude.
///
/// # Panics
///
/// This function does not panic.
#[must_use]
pub fn normalize(values: &[f32]) -> Option<NormalizedVector> {
    if values.is_empty() {
        return None;
    }

    let mut squared_magnitude = 0.0f64;
    for value in values {
        let value = f64::from(*value);
        if !value.is_finite() {
            return None;
        }
        squared_magnitude += value * value;
        if !squared_magnitude.is_finite() {
            return None;
        }
    }

    let magnitude = squared_magnitude.sqrt();
    if !magnitude.is_finite() {
        return None;
    }

    let values = if magnitude == 0.0 {
        vec![0.0; values.len()]
    } else {
        values
            .iter()
            .map(|value| {
                let normalized = f64::from(*value) / magnitude;
                normalized.to_string().parse::<f32>().ok()
            })
            .collect::<Option<Vec<_>>>()?
    };

    Some(NormalizedVector { values, magnitude })
}

#[must_use]
pub fn dot(left: &[f32], right: &[f32]) -> f64 {
    simd::dot(left, right)
}

#[must_use]
pub fn cosine_distance(left: &[f32], right: &[f32]) -> f64 {
    if left.is_empty() || right.is_empty() || left.len() != right.len() {
        return 1.0;
    }

    1.0 - dot(left, right)
}

#[must_use]
pub fn dot_distance_from_normalized_target(
    query: &[f32],
    normalized_target: &[f32],
    target_magnitude: f64,
) -> f64 {
    if query.is_empty() || normalized_target.is_empty() || query.len() != normalized_target.len() {
        return 0.0;
    }

    -dot(query, normalized_target) * target_magnitude
}

#[must_use]
pub fn cosine_distance_from_normalized_query(
    normalized_query: &[f32],
    normalized_target: &[f32],
) -> f64 {
    if normalized_query.is_empty()
        || normalized_target.is_empty()
        || normalized_query.len() != normalized_target.len()
    {
        return 1.0;
    }

    1.0 - dot(normalized_query, normalized_target)
}
