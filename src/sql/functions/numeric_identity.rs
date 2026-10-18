//! Preserve unimplemented NUMERIC identity at selected FLOAT8 boundaries.
use crate::sql::ast::{CastIdentity, Expr};
use crate::types::DataType;

pub(crate) fn has_unselected_numeric(expression: &Expr) -> bool {
    expression.any_descendant_or_self(&mut |node| {
        matches!(
            node,
            Expr::Cast {
                identity: CastIdentity::Numeric,
                ..
            }
        )
    }) || uncast_literal(expression)
}
fn uncast_literal(expression: &Expr) -> bool {
    if matches!(
        expression,
        Expr::Cast {
            data_type: DataType::Float,
            identity: CastIdentity::Scalar,
            ..
        }
    ) {
        return false;
    }
    if matches!(expression, Expr::NumericLiteral(_) | Expr::NumberLiteral(_)) {
        return true;
    }
    let mut pending = false;
    expression.for_each_child(|child| pending |= uncast_literal(child));
    pending
}
