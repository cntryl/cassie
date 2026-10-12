//! Resolved primitive SQL operator inputs, independent of general value keys.

use super::ast::{BinaryOp, Expr};
use crate::types::DataType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericComparisonDomain {
    Exact,
    Float8,
}

impl NumericComparisonDomain {
    pub(crate) fn resolve(left: &DataType, right: &DataType) -> Self {
        let integer = |data_type: &DataType| {
            matches!(
                data_type,
                DataType::SmallInt | DataType::Int | DataType::BigInt
            )
        };
        if (integer(left) && *right == DataType::Float)
            || (*left == DataType::Float && integer(right))
        {
            Self::Float8
        } else {
            Self::Exact
        }
    }
}

/// Recognizes native column equality and the binder's paired input coercions.
/// Explicit SQL casts and arbitrary value expressions retain scalar execution.
pub(crate) fn equality_columns(expression: &Expr) -> Option<(&str, &str, NumericComparisonDomain)> {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expression
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expr::Column(left), Expr::Column(right)) => {
            Some((left, right, NumericComparisonDomain::Exact))
        }
        (left, right) => {
            let Expr::Column(left) = super::binder::generated_float_coercion_operand(left)? else {
                return None;
            };
            let Expr::Column(right) = super::binder::generated_float_coercion_operand(right)?
            else {
                return None;
            };
            Some((left, right, NumericComparisonDomain::Float8))
        }
    }
}

/// A narrow field-index candidate from the binder's paired input conversions.
/// The runtime must still verify the field's catalog type before reading keys:
/// callers can manually construct the same public AST shape for an integer field.
pub(crate) fn generated_float_literal_constraint<'a>(
    left: &'a Expr,
    op: &BinaryOp,
    right: &'a Expr,
) -> Option<(&'a str, BinaryOp, f64)> {
    let left = super::binder::generated_float_coercion_operand(left)?;
    let right = super::binder::generated_float_coercion_operand(right)?;
    let (field, integer, op) = match (left, right) {
        (Expr::Column(field), Expr::IntegerLiteral(integer)) => (field, integer, op.clone()),
        (Expr::IntegerLiteral(integer), Expr::Column(field)) => {
            let reversed = match op {
                BinaryOp::Eq => BinaryOp::Eq,
                BinaryOp::Lt => BinaryOp::Gt,
                BinaryOp::Lte => BinaryOp::Gte,
                BinaryOp::Gt => BinaryOp::Lt,
                BinaryOp::Gte => BinaryOp::Lte,
                _ => return None,
            };
            (field, integer, reversed)
        }
        _ => return None,
    };
    matches!(
        op,
        BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte
    )
    .then(|| {
        (
            field.as_str(),
            op,
            crate::types::numeric::i64_to_f64(*integer),
        )
    })
}
