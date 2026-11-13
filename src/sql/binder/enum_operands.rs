//! Operand typing for named enum comparisons (#827).
//!
//! An unknown-typed string literal compared with an enum operand is that
//! enum's input (`status = 'open'`), so it is cast to the enum and a bad label
//! is `22P02`. A `text`-typed operand or a different enum type has no
//! operator (`42883`).
use super::coalesce_results::ResultTypes;
use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::{BinaryOp, CastIdentity, Expr};
use crate::types::{DataType, EnumRef};

fn descriptor(data_type: Option<&DataType>) -> Option<EnumRef> {
    match data_type?.flattened() {
        DataType::Enum(descriptor) => Some(descriptor.clone()),
        _ => None,
    }
}

const fn symbol(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::NotEq => "<>",
        BinaryOp::Lt => "<",
        BinaryOp::Lte => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Gte => ">=",
        _ => "=",
    }
}

fn pg_name(data_type: &DataType) -> String {
    match data_type.flattened() {
        DataType::Int => "integer".to_owned(),
        DataType::Float => "double precision".to_owned(),
        DataType::Varchar { .. } => "character varying".to_owned(),
        DataType::Char { .. } => "character".to_owned(),
        other => other.type_name(),
    }
}

fn undefined(op: &BinaryOp, left: &str, right: &str) -> CassieError {
    CassieError::from(
        DiagnosticError::new(
            "42883",
            format!("operator does not exist: {left} {} {right}", symbol(op)),
        )
        .with_hint(
            "No operator matches the given name and argument types. You might need to add explicit type casts.",
        ),
    )
}

/// Casts an unknown literal to `target`; rejects any other typed operand.
fn coerce(
    op: &BinaryOp,
    target: &EnumRef,
    other: &mut Expr,
    other_type: Option<&DataType>,
    enum_on_left: bool,
) -> Result<(), CassieError> {
    if matches!(other, Expr::StringLiteral(_)) {
        let literal = std::mem::replace(other, Expr::Null);
        *other = Expr::Cast {
            expr: Box::new(literal),
            data_type: DataType::Enum(target.clone()),
            source_type: None,
            identity: CastIdentity::Scalar,
        };
        return Ok(());
    }
    match other_type {
        // NULL and parameters of unresolved type adopt the enum.
        None | Some(DataType::Null) => Ok(()),
        Some(other_type) => {
            let other_name = pg_name(other_type);
            Err(if enum_on_left {
                undefined(op, &target.name, &other_name)
            } else {
                undefined(op, &other_name, &target.name)
            })
        }
    }
}

/// Qualifies the operands of a comparison involving an enum type.
///
/// # Errors
/// Returns `42883` for a typed non-enum operand or a different enum type.
pub(super) fn qualify_pair(
    op: &BinaryOp,
    left: &mut Expr,
    right: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    let left_type = types.expression_type(left);
    let right_type = types.expression_type(right);
    match (
        descriptor(left_type.as_ref()),
        descriptor(right_type.as_ref()),
    ) {
        (None, None) => Ok(()),
        (Some(left_enum), Some(right_enum)) if left_enum.oid == right_enum.oid => Ok(()),
        (Some(left_enum), Some(right_enum)) => {
            Err(undefined(op, &left_enum.name, &right_enum.name))
        }
        (Some(target), None) => coerce(op, &target, right, right_type.as_ref(), true),
        (None, Some(target)) => coerce(op, &target, left, left_type.as_ref(), false),
    }
}

#[cfg(test)]
#[path = "enum_operands_tests.rs"]
mod tests;
