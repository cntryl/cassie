use crate::types::Value;

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
        Value::Vector(value) => value
            .values
            .into_iter()
            .map(|component| {
                serde_json::Number::from_f64(f64::from(component))
                    .map(serde_json::Value::Number)
                    .ok_or(NON_FINITE_JSON_NUMBER)
            })
            .collect::<Result<Vec<_>, _>>()
            .map(serde_json::Value::Array),
        Value::Json(value) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::try_value_to_json;
    use crate::types::{Value, Vector};

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
