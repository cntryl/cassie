//! The prepared CHECK remains one owning field compared with one literal.
use crate::app::CassieError;
use crate::catalog::{ConstraintCheck, ConstraintOperator};
use crate::sql::ast::{BinaryOp, Expr, RowComparisonOp};
use crate::types::DataType;

pub(crate) fn validate(check: &ConstraintCheck) -> Result<(), CassieError> {
    let Some(expression) = check.bound_expression.as_deref() else {
        return if check.value.is_null() {
            Ok(())
        } else {
            Err(invalid())
        };
    };
    let Expr::Binary { left, op, right } = expression else {
        return Err(invalid());
    };
    let valid_op = matches!(
        (op, &check.operator),
        (
            BinaryOp::IsDistinctFrom
                | BinaryOp::JsonbCompare {
                    comparison: RowComparisonOp::IsDistinctFrom
                },
            ConstraintOperator::IsDistinctFrom
        ) | (
            BinaryOp::IsNotDistinctFrom
                | BinaryOp::JsonbCompare {
                    comparison: RowComparisonOp::IsNotDistinctFrom
                },
            ConstraintOperator::IsNotDistinctFrom
        )
    );
    if !valid_op
        || field_type(left, &check.field).is_none()
        || !literal_matches(right, &check.value)
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn field_type<'a>(expression: &'a Expr, field: &str) -> Option<&'a DataType> {
    fn find<'a>(expression: &'a Expr, field: &str, depth: usize) -> Option<&'a DataType> {
        if depth > 8 {
            return None;
        }
        if let Some(original) = super::binder::generated_float_coercion_operand(expression) {
            return find(original, field, depth + 1);
        }
        if let Expr::Cast {
            expr, data_type, ..
        } = expression
        {
            if matches!(expr.as_ref(),Expr::Column(name) if name==field) {
                return Some(data_type);
            }
            return find(expr, field, depth + 1);
        }
        None
    }
    find(expression, field, 0)
}

pub(crate) fn literal(expression: &Expr) -> Option<serde_json::Value> {
    fn find(expression: &Expr, depth: usize) -> Option<serde_json::Value> {
        if depth > 8 {
            return None;
        }
        if let Some(original) = super::binder::generated_float_coercion_operand(expression) {
            return find(original, depth + 1);
        }
        match expression {
            Expr::Cast { expr, .. } => find(expr, depth + 1),
            Expr::Null => Some(serde_json::Value::Null),
            Expr::BoolLiteral(value) => Some((*value).into()),
            Expr::StringLiteral(value) => Some(value.clone().into()),
            Expr::IntegerLiteral(value) => Some((*value).into()),
            Expr::NumberLiteral(value) => {
                serde_json::Number::from_f64(*value).map(serde_json::Value::Number)
            }
            Expr::NumericLiteral(value) => serde_json::from_str(value).ok(),
            _ => None,
        }
    }
    find(expression, 0)
}
fn invalid() -> CassieError {
    CassieError::Unsupported(
        "versioned CHECK requires its bound simple field/literal comparison".into(),
    )
}

/// Exact metadata identity for the admitted prepared shape, including cast
/// source provenance and floating literal bits. Other ASTs cannot reuse it.
pub(crate) fn exact_eq(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Column(left), Expr::Column(right))
        | (Expr::StringLiteral(left), Expr::StringLiteral(right))
        | (Expr::NumericLiteral(left), Expr::NumericLiteral(right)) => left == right,
        (Expr::Null, Expr::Null) => true,
        (Expr::BoolLiteral(left), Expr::BoolLiteral(right)) => left == right,
        (Expr::IntegerLiteral(left), Expr::IntegerLiteral(right)) => left == right,
        (Expr::NumberLiteral(left), Expr::NumberLiteral(right)) => {
            left.to_bits() == right.to_bits()
        }
        (
            Expr::Cast {
                expr: left,
                data_type: lt,
                source_type: ls,
                identity: li,
            },
            Expr::Cast {
                expr: right,
                data_type: rt,
                source_type: rs,
                identity: ri,
            },
        ) => lt == rt && ls == rs && li == ri && exact_eq(left, right),
        (
            Expr::Binary {
                left: ll,
                op: lo,
                right: lr,
            },
            Expr::Binary {
                left: rl,
                op: ro,
                right: rr,
            },
        ) => lo == ro && exact_eq(ll, rl) && exact_eq(lr, rr),
        (
            Expr::Case {
                operand: None,
                branches: lb,
                else_expr: Some(left),
            },
            Expr::Case {
                operand: None,
                branches: rb,
                else_expr: Some(right),
            },
        ) if lb.is_empty() && rb.is_empty() => exact_eq(left, right),
        _ => false,
    }
}

fn literal_matches(expression: &Expr, expected: &serde_json::Value) -> bool {
    fn leaf(expression: &Expr, depth: usize) -> Option<&Expr> {
        if depth > 8 {
            return None;
        }
        if let Some(original) = super::binder::generated_float_coercion_operand(expression) {
            return leaf(original, depth + 1);
        }
        if let Expr::Cast { expr, .. } = expression {
            leaf(expr, depth + 1)
        } else {
            Some(expression)
        }
    }
    match (leaf(expression, 0), expected) {
        (Some(Expr::Null), serde_json::Value::Null) => true,
        (Some(Expr::BoolLiteral(left)), serde_json::Value::Bool(right)) => left == right,
        (Some(Expr::StringLiteral(left)), serde_json::Value::String(right)) => left == right,
        (Some(Expr::IntegerLiteral(left)), serde_json::Value::Number(right)) => {
            right.as_i64() == Some(*left)
        }
        (Some(Expr::NumberLiteral(left)), serde_json::Value::Number(right)) => {
            right.is_f64()
                && right
                    .as_f64()
                    .is_some_and(|right| left.to_bits() == right.to_bits())
        }
        (Some(Expr::NumericLiteral(left)), serde_json::Value::Number(right)) => {
            left == right.as_str()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn should_reject_shapes_outside_the_prepared_simple_comparison() {
        // Arrange
        let cases = [
            ("CAST(n AS BIGINT) IS NOT DISTINCT FROM 7", true),
            ("CAST(m AS BIGINT) IS NOT DISTINCT FROM 7", false),
            ("CAST(n AS BIGINT) IS NOT DISTINCT FROM 8", false),
            ("CAST(n AS BIGINT) IS DISTINCT FROM 7", false),
            ("ABS(n) IS NOT DISTINCT FROM 7", false),
            ("CAST(n+1 AS BIGINT) IS NOT DISTINCT FROM 7", false),
            ("CAST(n AS BIGINT) IS NOT DISTINCT FROM ABS(7)", false),
        ];
        for (sql, accepted) in cases {
            let check = ConstraintCheck {
                field: "n".into(),
                operator: ConstraintOperator::IsNotDistinctFrom,
                value: 7.into(),
                bound_expression: Some(Box::new(
                    crate::sql::parser::parse_expression(sql).expect("expression"),
                )),
            };
            // Act
            let result = validate(&check);
            // Assert
            assert_eq!(result.is_ok(), accepted, "{sql}: {result:?}");
        }
    }
    #[test]
    fn should_distinguish_cast_provenance_in_check_metadata_identity() {
        // Arrange
        let left = crate::sql::parser::parse_expression("CAST(n AS BIGINT) IS NOT DISTINCT FROM 7")
            .expect("simple check");
        let mut right = left.clone();
        let Expr::Binary { left: operand, .. } = &mut right else {
            panic!("comparison");
        };
        let Expr::Cast { source_type, .. } = operand.as_mut() else {
            panic!("cast");
        };
        *source_type = Some(DataType::Float);
        // Act
        let unchanged = exact_eq(&left, &left.clone());
        let altered = exact_eq(&left, &right);
        // Assert
        assert!(unchanged);
        assert!(!altered);
    }
}
