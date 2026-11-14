//! Unknown literals that are the operand of a network-address operator.
//!
//! PostgreSQL resolves an unknown string literal to the type of the other
//! operand, so `ip << '10.0.0.0/8'` compares two `inet` values. The binder
//! casts the literal so the operator sees the family it was written for.

use super::coalesce_results::ResultTypes;
use super::{DataType, Expr};
use crate::sql::ast::CastIdentity;
use crate::types::extended::ExtendedType;

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

/// Casts a string literal operand to the network type of the other operand.
pub(super) fn qualify(left: &mut Expr, right: &mut Expr, types: &ResultTypes) {
    if let Some(target) = network_type(types.expression_type(left)) {
        cast_literal(right, target);
    } else if let Some(target) = network_type(types.expression_type(right)) {
        cast_literal(left, target);
    }
}
