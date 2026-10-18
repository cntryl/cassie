//! Floating unary signs retain IEEE signed zero; integers must bind a width first.
use super::{QueryError, ScalarValue};
use crate::sql::ast::UnaryNumericOp;

pub(super) fn evaluate(value: &ScalarValue, op: UnaryNumericOp) -> Result<ScalarValue, QueryError> {
    if matches!(op, UnaryNumericOp::SquareRoot | UnaryNumericOp::CubeRoot) {
        if matches!(value, ScalarValue::Null) {
            return Ok(ScalarValue::Null);
        }
        let number = match value {
            ScalarValue::Int(number) => crate::types::numeric::i64_to_f64(*number),
            ScalarValue::Float(number) => *number,
            _ => {
                return Err(QueryError::General(
                    "operator does not exist: unary math with non-numeric operand".into(),
                ))
            }
        };
        if op == UnaryNumericOp::SquareRoot && number < 0.0 {
            return Err(QueryError::General(
                "cannot take square root of a negative number".into(),
            ));
        }
        return Ok(ScalarValue::Float(if number == 0.0 {
            // PostgreSQL's dsqrt/dcbrt return positive zero for either zero sign.
            0.0
        } else if op == UnaryNumericOp::SquareRoot {
            number.sqrt()
        } else {
            number.cbrt()
        }));
    }
    if op == UnaryNumericOp::Complement {
        return match value {
            ScalarValue::Int(value) => Ok(ScalarValue::Int(!value)),
            ScalarValue::Typed(typed) => crate::types::extended::operations::complement(typed)
                .map(ScalarValue::Typed)
                .ok_or_else(|| {
                    QueryError::General("operator does not exist: ~ non-integer".into())
                }),
            ScalarValue::Null => Ok(ScalarValue::Null),
            _ => Err(QueryError::General(
                "operator does not exist: ~ non-integer".into(),
            )),
        };
    }
    match value {
        ScalarValue::Null => Ok(ScalarValue::Null),
        ScalarValue::Float(value) => Ok(ScalarValue::Float(match op {
            UnaryNumericOp::Minus => -*value,
            UnaryNumericOp::Absolute => value.abs(),
            _ => *value,
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
