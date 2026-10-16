//! Floating unary signs retain IEEE signed zero; integers must bind a width first.
use super::{QueryError, ScalarValue};
use crate::sql::ast::UnaryNumericOp;

pub(super) fn evaluate(value: &ScalarValue, op: UnaryNumericOp) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Null => Ok(ScalarValue::Null),
        ScalarValue::Float(value) => Ok(ScalarValue::Float(if op == UnaryNumericOp::Minus {
            -*value
        } else {
            *value
        })),
        _ => Err(QueryError::Cassie(crate::app::CassieError::Unsupported(
            "numeric unary operator requires a resolved operand signature".into(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_preserve_zero_sign_given_float_operands_when_prefix_signs_are_evaluated() {
        // Arrange
        let zero = ScalarValue::Float(0.0);
        // Act
        let minus = evaluate(&zero, UnaryNumericOp::Minus).expect("minus");
        let plus = evaluate(&zero, UnaryNumericOp::Plus).expect("plus");
        // Assert
        assert!(
            matches!(minus, ScalarValue::Float(value) if value.to_bits() == (-0.0f64).to_bits())
        );
        assert!(matches!(plus, ScalarValue::Float(value) if value.to_bits() == 0.0f64.to_bits()));
    }
    #[test]
    fn should_reject_unresolved_integer_width_given_raw_unary_nodes_when_runtime_cannot_select_a_signature(
    ) {
        // Arrange
        let value = ScalarValue::Int(-2_147_483_648);
        // Act
        let result = evaluate(&value, UnaryNumericOp::Minus);
        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(crate::app::CassieError::Unsupported(_)))
        ));
    }
}
