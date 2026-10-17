//! Selected scalar binary kernels.
use super::{
    cast_to_text, checked_math_result, comparison_result, eq_value, integers, like_match,
    ordered_cmp, vector_distance, BinaryOp, QueryError, ScalarValue,
};

pub(super) fn binary_scalar(
    left: &ScalarValue,
    op: &BinaryOp,
    right: &ScalarValue,
) -> Result<ScalarValue, QueryError> {
    let result = match op {
        BinaryOp::Concat => match (cast_to_text(left), cast_to_text(right)) {
            (Some(mut left), Some(right)) => {
                left.push_str(&right);
                ScalarValue::Str(left)
            }
            _ => ScalarValue::Null,
        },
        BinaryOp::And => left
            .predicate_result()?
            .and(right.predicate_result()?)
            .into_scalar(),
        BinaryOp::Or => left
            .predicate_result()?
            .or(right.predicate_result()?)
            .into_scalar(),
        BinaryOp::Eq => comparison_result(eq_value(left, right)),
        BinaryOp::IsDistinctFrom | BinaryOp::IsNotDistinctFrom => {
            let equal = match (left, right) {
                (ScalarValue::Null, ScalarValue::Null) => true,
                (ScalarValue::Null, _) | (_, ScalarValue::Null) => false,
                _ => eq_value(left, right).ok_or_else(|| {
                    QueryError::General("incompatible null-safe comparison operands".to_string())
                })?,
            };
            ScalarValue::Bool(if matches!(op, BinaryOp::IsNotDistinctFrom) {
                equal
            } else {
                !equal
            })
        }
        BinaryOp::NotEq => comparison_result(eq_value(left, right).map(|value| !value)),
        BinaryOp::Lt => comparison_result(ordered_cmp(left, right, std::cmp::Ordering::is_lt)),
        BinaryOp::Lte => comparison_result(ordered_cmp(left, right, |ordering| !ordering.is_gt())),
        BinaryOp::Gt => comparison_result(ordered_cmp(left, right, std::cmp::Ordering::is_gt)),
        BinaryOp::Gte => comparison_result(ordered_cmp(left, right, |ordering| !ordering.is_lt())),
        BinaryOp::Like => comparison_result(like_match(left.as_str(), right.as_str())?),
        BinaryOp::Integer { operation, width } => {
            integers::arithmetic(left, *operation, *width, right)?
        }
        BinaryOp::Add => checked_math_result(left, right, i64::checked_add, |a, b| a + b)?,
        BinaryOp::Sub => checked_math_result(left, right, i64::checked_sub, |a, b| a - b)?,
        BinaryOp::Mul => checked_math_result(left, right, i64::checked_mul, |a, b| a * b)?,
        BinaryOp::Power => {
            if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) { ScalarValue::Null }
            else {
                let left = left.to_f64().ok_or_else(|| QueryError::General("power requires numeric inputs".into()))?;
                let right = right.to_f64().ok_or_else(|| QueryError::General("power requires numeric inputs".into()))?;
                ScalarValue::Float(super::functions::math::power(left, right)?)
            }
        }
        BinaryOp::Bitwise { .. } => return Err(crate::app::CassieError::Unsupported("bitwise operator requires resolved existing integer operands".into()).into()),
        BinaryOp::Modulo => return Err(crate::app::CassieError::Unsupported("remainder requires selected existing integer operands; NUMERIC remains pending under #793/#812".into()).into()),
        BinaryOp::Div => {
            if right.to_f64().is_some_and(|value| value == 0.0) && left.to_f64().is_some() {
                return Err(QueryError::General("division by zero".to_string()));
            }
            // Integer operands divide with truncation toward zero, as in
            // PostgreSQL; `i64::MIN / -1` is the one overflowing quotient.
            checked_math_result(left, right, i64::checked_div, |a, b| a / b)?
        }
        BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
            if matches!(left, ScalarValue::Null) || matches!(right, ScalarValue::Null) {
                ScalarValue::Null
            } else {
                ScalarValue::Float(vector_distance(op, left, right)?)
            }
        }
    };
    Ok(result)
}
