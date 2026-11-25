//! Strict Boolean truth predicates produce a non-NULL Boolean result.
use super::{QueryError, ScalarValue};
use crate::sql::ast::TruthValue;

pub(super) fn evaluate(
    value: &ScalarValue,
    truth: TruthValue,
    negated: bool,
) -> Result<ScalarValue, QueryError> {
    let input = match value {
        ScalarValue::Bool(value) => Some(*value),
        ScalarValue::Null => None,
        _ => {
            return Err(QueryError::General(format!(
                "{} operand must be BOOLEAN",
                truth.label(negated)
            )))
        }
    };
    Ok(ScalarValue::Bool(truth.evaluate(input, negated)))
}
