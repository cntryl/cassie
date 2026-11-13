//! Operand typing for named enum comparisons (#827).
//!
//! An unknown-typed string literal compared with an enum operand is that
//! enum's input (`status = 'open'`), so it is cast to the enum and a bad label
//! is `22P02`. A `text`-typed operand or a different enum type has no
//! operator (`42883`).
use super::super::coalesce_results::ResultTypes;
use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::{BinaryOp, CastIdentity, Expr};
use crate::types::array::ParsedTextArrayElement;
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
    if let Expr::StringLiteral(label) = other {
        // `enum_in` runs when PostgreSQL resolves the literal, so a bad label
        // fails even when no row reaches the comparison.
        input_label(target, label)?;
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

/// Validates the labels of an unknown array literal on the right of
/// `enum op ANY|ALL ('{...}')`, which PostgreSQL resolves as the enum's array
/// input. A malformed literal is left for the evaluator's array diagnostic.
///
/// # Errors
/// Returns `22P02` for an invalid label and `55P04` for an uncommitted one.
pub(super) fn qualify_quantified(
    op: &BinaryOp,
    left: &Expr,
    right: &Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    let (BinaryOp::Quantified { .. }, Expr::StringLiteral(literal)) = (op, right) else {
        return Ok(());
    };
    let Some(target) = descriptor(types.expression_type(left).as_ref()) else {
        return Ok(());
    };
    validate_array_labels(&target, literal)
}

fn validate_array_labels(target: &EnumRef, literal: &str) -> Result<(), CassieError> {
    // Split the literal as text so each label meets `enum_in` here, with its
    // own SQLSTATE, rather than the array parser's untyped message.
    let Ok(parsed) = crate::types::array::parse_sql_text_array(literal, &DataType::Text) else {
        return Ok(());
    };
    for element in &parsed.flat_elements {
        if let ParsedTextArrayElement::Value(serde_json::Value::String(label)) = element {
            input_label(target, label)?;
        }
    }
    Ok(())
}

fn input_label(target: &EnumRef, label: &str) -> Result<(), CassieError> {
    crate::types::EnumValue::parse_label(target, label)
        .map(|_| ())
        .map_err(|error| {
            CassieError::from(crate::executor::filter::enum_cast::input_diagnostic(&error))
        })
}

#[cfg(test)]
#[path = "enum_operands_tests.rs"]
mod tests;
