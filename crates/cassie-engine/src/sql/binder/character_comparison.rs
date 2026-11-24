//! Resolve bpchar comparison coercions without discarding projected padding.
use super::coalesce_results::ResultTypes;
use crate::sql::ast::{BinaryOp, CastIdentity, Expr};
use crate::types::DataType;

pub(super) fn qualify(op: &BinaryOp, left: &mut Expr, right: &mut Expr, types: &ResultTypes) {
    if !matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IsDistinctFrom
            | BinaryOp::IsNotDistinctFrom
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
    ) {
        return;
    }
    let left_type = types.expression_type(left);
    let right_type = types.expression_type(right);
    if !matches!(left_type, Some(DataType::Char { .. }))
        && !matches!(right_type, Some(DataType::Char { .. }))
    {
        return;
    }
    // A typed TEXT operand selects text comparison: its blanks stay significant.
    // Unknown literals and VARCHAR use the bpchar signature and ignore blanks.
    let text = matches!(left_type, Some(DataType::Text)) && !matches!(left, Expr::StringLiteral(_))
        || matches!(right_type, Some(DataType::Text)) && !matches!(right, Expr::StringLiteral(_));
    canonicalize(left, left_type.as_ref(), text);
    canonicalize(right, right_type.as_ref(), text);
}

fn canonicalize(expr: &mut Expr, data_type: Option<&DataType>, text: bool) {
    let character = matches!(data_type, Some(DataType::Char { .. }));
    let coerced = !text
        && (matches!(data_type, Some(DataType::Varchar { .. }))
            || matches!(expr, Expr::StringLiteral(_)));
    if !character && !coerced {
        return;
    }
    let mut value = std::mem::replace(expr, Expr::Null);
    if coerced {
        value = Expr::Cast {
            expr: Box::new(value),
            data_type: DataType::Char { length: None },
            source_type: data_type.cloned(),
            identity: CastIdentity::Scalar,
        };
    }
    *expr = Expr::Cast {
        expr: Box::new(value),
        data_type: DataType::Text,
        source_type: Some(DataType::Char { length: None }),
        identity: CastIdentity::Scalar,
    };
}
