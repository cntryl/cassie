//! Resolve JSONB containment operators through the existing typed operator family.
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::{extended::TypedOperator, DataType};

pub(crate) const fn operator_symbol(op: &BinaryOp) -> Option<&'static str> {
    match op.typed_operator() {
        Some(TypedOperator::ContainsOrEqual) => Some("@>"),
        Some(TypedOperator::ContainedOrEqual) => Some("<@"),
        _ => None,
    }
}

pub(super) fn qualify(
    left: &mut Expr,
    op: &BinaryOp,
    right: &mut Expr,
    left_type: Option<&DataType>,
    right_type: Option<&DataType>,
) {
    if operator_symbol(op).is_none() {
        return;
    }
    if left_type.is_some_and(DataType::is_json) && matches!(right, Expr::StringLiteral(_)) {
        cast_json(right, left_type.cloned().unwrap_or(DataType::Jsonb));
    } else if right_type.is_some_and(DataType::is_json) && matches!(left, Expr::StringLiteral(_)) {
        cast_json(left, right_type.cloned().unwrap_or(DataType::Jsonb));
    }
}

fn cast_json(expression: &mut Expr, data_type: DataType) {
    *expression = Expr::Cast {
        expr: Box::new(std::mem::replace(expression, Expr::Null)),
        data_type,
        source_type: None,
        identity: crate::sql::ast::CastIdentity::Scalar,
    };
}
