//! Binder-internal FLOAT normalization, distinct from a SQL CAST boundary.
//!
//! SQL parsing requires at least one CASE WHEN. A searched CASE with no branches
//! and one ELSE is therefore a private coercion wrapper: evaluation visits the
//! original operand once, while output provenance follows that operand. This
//! convention does not restrict manually constructed public Expr::Case values.
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
    }
}

pub(crate) fn original_operand(expression: &Expr) -> Option<&Expr> {
    let Expr::Cast {
        expr,
        data_type: DataType::Float,
    } = expression
    else {
        return None;
    };
    let Expr::Case {
        operand: None,
        branches,
        else_expr: Some(original),
    } = expr.as_ref()
    else {
        return None;
    };
    branches.is_empty().then_some(original.as_ref())
}
