//! Typed evaluator boundary for persisted scalar expression indexes.

use cassie_core::types::Value;

/// Evaluates one stored scalar-index expression against a typed row.
pub trait ScalarExpressionEvaluator {
    /// The evaluator's owning-layer error type.
    type Error;

    /// Evaluates the serialized expression without exposing its syntax here.
    /// # Errors
    /// Returns the evaluator error or reports that a value cannot be represented as a scalar index key.
    fn evaluate(
        &self,
        stored_expression: &str,
        row: &[(String, Value)],
    ) -> Result<Value, Self::Error>;
}

/// Preserves either the engine evaluator failure or a storage-key conversion failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScalarExpressionEvaluationError<E> {
    /// The engine-owned evaluator rejected or failed to evaluate the expression.
    Evaluation(E),
    /// The evaluated value cannot be represented as the persisted JSON key component.
    UnsupportedValue(String),
}

/// Evaluates stored expressions and applies the scalar-index NULL uniqueness policy.
/// # Errors
/// Returns the evaluator error or reports that a value cannot be represented as a scalar index key.
pub fn evaluate_expression_index_values<Evaluator>(
    expressions: &[String],
    row: &[(String, Value)],
    nulls_not_distinct: bool,
    evaluator: &Evaluator,
) -> Result<Option<Vec<serde_json::Value>>, ScalarExpressionEvaluationError<Evaluator::Error>>
where
    Evaluator: ScalarExpressionEvaluator,
{
    let mut values = Vec::with_capacity(expressions.len());
    for expression in expressions {
        let value = evaluator
            .evaluate(expression, row)
            .map_err(ScalarExpressionEvaluationError::Evaluation)?;
        if matches!(value, Value::Null) && !nulls_not_distinct {
            return Ok(None);
        }
        values
            .push(value_to_json(value).map_err(ScalarExpressionEvaluationError::UnsupportedValue)?);
    }
    Ok(Some(values))
}

fn value_to_json(value: Value) -> Result<serde_json::Value, String> {
    match value {
        Value::Null => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(value)),
        Value::Int64(value) => Ok(serde_json::Value::Number(value.into())),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(unsupported_non_finite_value),
        Value::String(value) => Ok(serde_json::Value::String(value)),
        Value::Array(value) => Ok(value.to_storage_json()),
        Value::Typed(value) => Ok(value.to_json()),
        Value::Vector(value) => value
            .values
            .into_iter()
            .map(|value| {
                serde_json::Number::from_f64(f64::from(value))
                    .map(serde_json::Value::Number)
                    .ok_or_else(unsupported_non_finite_value)
            })
            .collect::<Result<_, _>>()
            .map(serde_json::Value::Array),
        Value::Json(value) => Ok(value),
    }
}

fn unsupported_non_finite_value() -> String {
    "non-finite scalar index expression values are not supported".to_owned()
}
