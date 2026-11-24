use super::{
    evaluate_expression_index_values, ScalarExpressionEvaluationError, ScalarExpressionEvaluator,
};
use cassie_core::types::Value;

struct Evaluator;

impl ScalarExpressionEvaluator for Evaluator {
    type Error = &'static str;

    fn evaluate(&self, expression: &str, row: &[(String, Value)]) -> Result<Value, Self::Error> {
        match expression {
            "integer" => Ok(row[0].1.clone()),
            "null" => Ok(Value::Null),
            "failure" => Err("evaluation failed"),
            _ => Err("unknown expression"),
        }
    }
}

#[test]
fn should_preserve_typed_expression_values_and_null_uniqueness_policy() {
    // Arrange
    let expressions = ["integer".to_owned(), "null".to_owned()];
    let row = [("n".to_owned(), Value::Int64(7))];

    // Act
    let distinct = evaluate_expression_index_values(&expressions, &row, true, &Evaluator);
    let nulls_distinct = evaluate_expression_index_values(&expressions, &row, false, &Evaluator);

    // Assert
    assert_eq!(
        distinct,
        Ok(Some(vec![serde_json::json!(7), serde_json::Value::Null]))
    );
    assert_eq!(nulls_distinct, Ok(None));
}

#[test]
fn should_preserve_typed_expression_evaluator_errors() {
    // Arrange
    let expressions = ["failure".to_owned()];
    let row = [("n".to_owned(), Value::Int64(7))];

    // Act
    let result = evaluate_expression_index_values(&expressions, &row, true, &Evaluator);

    // Assert
    assert!(matches!(
        result,
        Err(ScalarExpressionEvaluationError::Evaluation(
            "evaluation failed"
        ))
    ));
}
