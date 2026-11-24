//! Scalar functions with result or execution workspace owned by the query.

use super::ProjectionOp;
use crate::sql::ast::{BinaryOp, Expr};

pub(super) fn requires_owner(ops: &[ProjectionOp]) -> bool {
    ops.iter().any(|op| match op {
        ProjectionOp::ScalarFunction { expr, .. } => {
            expr.any_descendant_or_self(&mut expression_requires_owner)
        }
        _ => false,
    })
}

fn expression_requires_owner(expr: &Expr) -> bool {
    match expr {
        Expr::Function(function) => {
            let name = function.name.rsplit('.').next().unwrap_or(&function.name);
            ["power", "pow", "to_char", "to_date", "to_timestamp"]
                .iter()
                .any(|candidate| name.eq_ignore_ascii_case(candidate))
        }
        Expr::Binary {
            op: BinaryOp::Power,
            ..
        } => true,
        _ => false,
    }
}
