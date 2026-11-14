//! Unknown literals that are the operand of a network-address operator.
//!
//! PostgreSQL resolves an unknown string literal to the type of the other
//! operand, so `ip << '10.0.0.0/8'` compares two `inet` values. The binder
//! casts the literal so the operator sees the family it was written for.

use super::coalesce_results::ResultTypes;
use super::{BinaryOp, DataType, Expr};
use crate::sql::ast::CastIdentity;
use crate::types::extended::{ExtendedType, TypedOperator};

fn network_type(data_type: Option<DataType>) -> Option<DataType> {
    data_type.filter(|kind| {
        matches!(
            kind,
            DataType::Extended(
                ExtendedType::Inet
                    | ExtendedType::Cidr
                    | ExtendedType::Macaddr
                    | ExtendedType::Macaddr8
            )
        )
    })
}

fn cast_literal(literal: &mut Expr, target: DataType) {
    if !matches!(literal, Expr::StringLiteral(_)) {
        return;
    }
    let value = std::mem::replace(literal, Expr::Null);
    *literal = Expr::Cast {
        expr: Box::new(value),
        data_type: target,
        source_type: None,
        identity: CastIdentity::Scalar,
    };
}

/// The type an unknown literal takes beside `other` under `op`, following
/// PostgreSQL's preference for an operator with both inputs of `other`'s type.
fn literal_type(op: &BinaryOp, other: DataType) -> Option<DataType> {
    match op.typed_operator() {
        // No `inet + inet` exists, so `inet + '1'` reads the literal as `bigint`.
        Some(TypedOperator::Add) => matches!(
            other,
            DataType::Extended(ExtendedType::Inet | ExtendedType::Cidr)
        )
        .then_some(DataType::BigInt),
        // `inet || 'x'` is text concatenation.
        Some(TypedOperator::Concat) => None,
        Some(_) => Some(other),
        None => matches!(
            op,
            BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::IsDistinctFrom
                | BinaryOp::IsNotDistinctFrom
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte
        )
        .then_some(other),
    }
}

/// Casts a string literal operand to the type PostgreSQL resolves for it
/// beside a network-typed operand.
pub(super) fn qualify(op: &BinaryOp, left: &mut Expr, right: &mut Expr, types: &ResultTypes) {
    if let Some(target) = network_type(types.expression_type(left)) {
        if let Some(target) = literal_type(op, target) {
            cast_literal(right, target);
        }
    } else if let Some(target) = network_type(types.expression_type(right)) {
        if let Some(target) = literal_type(op, target) {
            cast_literal(left, target);
        }
    }
}
