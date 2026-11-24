use std::cmp::Ordering;

use crate::types::Value;

mod estimated_controlled;
#[cfg(test)]
pub(crate) use estimated_controlled::cancel_next_estimate_frame;
mod jsonb;
mod jsonb_array;
mod record;
pub(crate) use jsonb::JsonbKey;
pub(crate) use jsonb_array::JsonbArrayKey;
pub(crate) use record::RecordKey;

pub(crate) use cassie_core::types::semantic::compare_sql_arrays;
pub(crate) type SemanticValue =
    cassie_core::types::semantic::SemanticValue<JsonbKey, JsonbArrayKey, RecordKey>;

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SemanticKey(Vec<SemanticValue>);

impl SemanticKey {
    #[cfg(test)]
    pub(crate) fn actual_retained_bytes_for_test(&self) -> usize {
        fn heap(value: &SemanticValue) -> usize {
            match value {
                SemanticValue::String(text) | SemanticValue::Json(text) => text.capacity(),
                SemanticValue::Vector(values) => values.capacity() * std::mem::size_of::<u32>(),
                SemanticValue::Array(values) => {
                    values.capacity() * std::mem::size_of::<SemanticValue>()
                        + values.iter().map(heap).sum::<usize>()
                }
                SemanticValue::SqlArray {
                    dimensions,
                    elements,
                } => {
                    dimensions.capacity() * std::mem::size_of::<(i32, i32)>()
                        + elements.capacity() * std::mem::size_of::<SemanticValue>()
                        + elements.iter().map(heap).sum::<usize>()
                }
                SemanticValue::Null
                | SemanticValue::Bool(_)
                | SemanticValue::Number(_)
                | SemanticValue::Typed(_)
                | SemanticValue::Jsonb(_)
                | SemanticValue::JsonbArray(_)
                | SemanticValue::Record(_) => 0,
            }
        }
        self.0.capacity() * std::mem::size_of::<SemanticValue>()
            + self.0.iter().map(heap).sum::<usize>()
    }

    pub(crate) fn from_semantic_values(values: Vec<SemanticValue>) -> Self {
        Self(values)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_values<'a>(values: impl IntoIterator<Item = &'a Value>) -> Self {
        Self(values.into_iter().map(SemanticValue::from_value).collect())
    }

    #[must_use]
    pub(crate) fn single(value: &Value) -> Self {
        Self(vec![SemanticValue::from_value(value)])
    }

    #[must_use]
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.0
            .iter()
            .map(SemanticValue::estimated_bytes)
            .sum::<usize>()
            .saturating_add(
                self.0
                    .len()
                    .saturating_mul(std::mem::size_of::<SemanticValue>()),
            )
    }
}

#[must_use]
pub(crate) fn compare_values(left: &Value, right: &Value) -> Ordering {
    cassie_core::types::semantic::compare_values(left, right)
}

#[must_use]
pub(crate) fn compare_numeric_values(left: &Value, right: &Value) -> Option<Ordering> {
    cassie_core::types::semantic::compare_numeric_values(left, right)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn should_compare_integer_float_boundaries_without_rounding() {
        // Arrange
        let exact = Value::Int64(9_007_199_254_740_993);
        let rounded = Value::Float64(9_007_199_254_740_992.0);

        // Act
        let ordering = compare_values(&exact, &rounded);

        // Assert
        assert_eq!(ordering, Ordering::Greater);
        assert_ne!(SemanticKey::single(&exact), SemanticKey::single(&rounded));
    }

    #[test]
    fn should_normalize_equivalent_numeric_representations() {
        // Arrange
        let integer = SemanticKey::single(&Value::Int64(1));
        let float = SemanticKey::single(&Value::Float64(1.0));
        let positive_zero = SemanticKey::single(&Value::Float64(0.0));
        let negative_zero = SemanticKey::single(&Value::Float64(-0.0));
        let mut numeric_keys = crate::types::semantic_index::KeyedSet::new(None).expect("index");

        // Act
        let integral_equal = integer == float;
        let zeros_equal = positive_zero == negative_zero;
        let inserted_integer = numeric_keys.insert(integer).expect("integer");
        let inserted_equal_float = numeric_keys.insert(float).expect("equal float");

        // Assert
        assert!(integral_equal);
        assert!(zeros_equal);
        assert!(inserted_integer);
        assert!(!inserted_equal_float);
        assert_eq!(numeric_keys.len(), 1);
    }

    #[test]
    fn should_order_equivalent_timestamp_formats_by_instant() {
        // Arrange
        let legacy_whole = Value::String("2024-01-01T12:00:00Z".to_string());
        let fixed_whole = Value::String("2024-01-01T12:00:00.000000Z".to_string());
        let fractional = Value::String("2024-01-01T12:00:00.500000Z".to_string());

        // Act
        let whole_before_fraction = compare_values(&legacy_whole, &fractional);
        let fraction_after_whole = compare_values(&fractional, &legacy_whole);
        let same_instant = SemanticKey::single(&legacy_whole) == SemanticKey::single(&fixed_whole);

        // Assert
        assert_eq!(whole_before_fraction, Ordering::Less);
        assert_eq!(fraction_after_whole, Ordering::Greater);
        assert!(same_instant);
    }

    #[test]
    fn should_match_core_ordering_for_portable_values() {
        // Arrange
        let values = [
            Value::Null,
            Value::Bool(false),
            Value::Int64(1),
            Value::Float64(1.5),
            Value::String("2024-01-01T12:00:00Z".into()),
            Value::String("2024-01-01T12:00:00.500000Z".into()),
            Value::Json(serde_json::json!({"a": 1, "b": [true, null]})),
            Value::Typed(crate::types::typed::TypedValue::Real(1.5)),
            Value::Typed(crate::types::typed::TypedValue::Numeric(
                crate::types::extended::numeric::Numeric::from_i64(2),
            )),
        ];

        // Act
        for left in &values {
            for right in &values {
                // Assert
                assert_eq!(
                    compare_values(left, right),
                    SemanticKey::single(left).cmp(&SemanticKey::single(right)),
                );
            }
        }
    }
}
