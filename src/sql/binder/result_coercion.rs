//! Binder-internal FLOAT normalization, distinct from a SQL CAST boundary.
//!
//! SQL parsing requires at least one CASE WHEN. A searched CASE with no branches
//! and one ELSE is therefore a private coercion wrapper: evaluation visits the
//! original operand once, while output provenance follows that operand. This
//! convention does not restrict manually constructed public `Expr::Case` values.
use super::{DataType, Expr};

pub(super) fn float(operand: Expr) -> Expr {
    if original_operand(&operand).is_some() {
        return operand;
    }
    Expr::Cast {
        expr: Box::new(Expr::Case {
            operand: None,
            branches: Vec::new(),
            else_expr: Some(Box::new(operand)),
        }),
        data_type: DataType::Float,
        source_type: None,
        identity: crate::sql::ast::CastIdentity::Scalar,
    }
}

pub(crate) fn original_operand(expression: &Expr) -> Option<&Expr> {
    let Expr::Cast {
        expr,
        data_type: DataType::Float,
        identity: crate::sql::ast::CastIdentity::Scalar,
        ..
    } = expression
    else {
        return None;
    };
    case_operand(expr)
}

pub(crate) fn case_operand(expression: &Expr) -> Option<&Expr> {
    let Expr::Case {
        operand: None,
        branches,
        else_expr: Some(original),
    } = expression
    else {
        return None;
    };
    branches.is_empty().then_some(original.as_ref())
}

#[cfg(test)]
#[path = "result_coercion_tests.rs"]
mod tests;
