use crate::types::Value;

/// Keep JSON-only scalar documents that the existing primitive carriers cannot preserve.
/// Ordinary signed/floating numeric and Boolean JSON scalar semantics stay unchanged.
pub(crate) fn requires_document_carrier(value: &serde_json::Value) -> bool {
    value.is_null()
        || value.is_string()
        || value
            .as_u64()
            .is_some_and(|number| i64::try_from(number).is_err())
}

pub(crate) const NON_FINITE_JSON_NUMBER: &str =
    "non-finite floating-point values cannot be represented in JSON";

pub(crate) fn try_value_to_json(value: Value) -> Result<serde_json::Value, &'static str> {
    match value {
        Value::Null => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(value)),
        Value::Int64(value) => Ok(serde_json::Value::Number(value.into())),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or(NON_FINITE_JSON_NUMBER),
        Value::String(value) => Ok(serde_json::Value::String(value)),
        Value::Vector(value) => {
            // Fallible iterator collection loses the exact size hint and can grow beyond
            // the component count reserved by controlled query output.
            let mut values = Vec::with_capacity(value.values.len());
            for component in value.values {
                let number = serde_json::Number::from_f64(f64::from(component))
                    .ok_or(NON_FINITE_JSON_NUMBER)?;
                values.push(serde_json::Value::Number(number));
            }
            Ok(serde_json::Value::Array(values))
        }
        Value::Json(value) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::try_value_to_json;
    use crate::types::{Value, Vector};

    #[test]
    fn should_bound_json_vector_storage_by_the_reserved_component_count() {
        // Arrange
        let dimensions = [1, 3, 8193];

        // Act
        let arrays = dimensions.map(|count| {
            let converted = try_value_to_json(Value::Vector(Vector::new(vec![1.0; count])))
                .expect("finite vector");
            let serde_json::Value::Array(values) = converted else {
                panic!("vector must produce a JSON array");
            };
            values
        });

        // Assert
        for (values, count) in arrays.iter().zip(dimensions) {
            assert_eq!(values.len(), count);
            assert!(
                values.capacity() <= count,
                "spare capacity exceeds the query reservation"
            );
        }
    }

    #[test]
    fn should_reject_nonfinite_float_values() {
        // Arrange
        let values = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];

        // Act
        let results = values.map(|value| try_value_to_json(Value::Float64(value)));

        // Assert
        assert!(results.iter().all(Result::is_err));
    }

    #[test]
    fn should_reject_nonfinite_vector_components_from_json_arrays() {
        // Arrange
        let value = Value::Vector(Vector::new(vec![1.0, f32::INFINITY]));

        // Act
        let result = try_value_to_json(value);

        // Assert
        assert!(result.is_err());
    }
}
