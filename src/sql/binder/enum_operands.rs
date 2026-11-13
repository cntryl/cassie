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

/// The element enum of an enum array type.
fn array_descriptor(data_type: Option<&DataType>) -> Option<EnumRef> {
    match data_type?.flattened() {
        DataType::Array(element) => descriptor(Some(element.as_ref())),
        _ => None,
    }
}

/// Casts an unknown literal compared with an enum array to that array type,
/// as PostgreSQL resolves `es = '{a,b}'`; any other operand is left alone.
fn coerce_array_literal(target: &EnumRef, other: &mut Expr) -> Result<(), CassieError> {
    let Expr::StringLiteral(literal) = other else {
        return Ok(());
    };
    validate_array_labels(target, literal)?;
    let literal = std::mem::replace(other, Expr::Null);
    *other = Expr::Cast {
        expr: Box::new(literal),
        data_type: DataType::Array(Box::new(DataType::Enum(target.clone()))),
        source_type: None,
        identity: CastIdentity::Scalar,
    };
    Ok(())
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
        (None, None) => {
            if let Some(target) = array_descriptor(left_type.as_ref()) {
                coerce_array_literal(&target, right)
            } else if let Some(target) = array_descriptor(right_type.as_ref()) {
                coerce_array_literal(&target, left)
            } else {
                Ok(())
            }
        }
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
    let BinaryOp::Quantified { comparison, .. } = op else {
        return Ok(());
    };
    let Some(target) = descriptor(types.expression_type(left).as_ref()) else {
        return Ok(());
    };
    match right {
        Expr::StringLiteral(literal) => validate_array_labels(&target, literal),
        // `ARRAY['a', 'b']` is `text[]`, which has no operator with an enum.
        Expr::Function(constructor)
            if constructor.name == crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION =>
        {
            match types.expression_type(right) {
                Some(DataType::Array(element))
                    if matches!(
                        element.flattened(),
                        DataType::Text
                            | DataType::Name
                            | DataType::Varchar { .. }
                            | DataType::Char { .. }
                    ) =>
                {
                    Err(undefined(comparison, &target.name, &pg_name(&element)))
                }
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
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

/// Whether the binder's static type of `expr` is reliable enough to refuse an
/// assignment: a column, an explicit cast, or a function of non-enum inputs.
fn statically_typed(expr: &Expr, types: &ResultTypes) -> bool {
    match expr {
        Expr::Column(_) | Expr::Cast { .. } => true,
        Expr::Function(function) => !function
            .args
            .iter()
            .any(|argument| descriptor(types.expression_type(argument).as_ref()).is_some()),
        _ => false,
    }
}

/// An enum column accepts an unknown literal, NULL, a parameter or a value of
/// the same enum. A `text`-typed expression or another enum has no assignment
/// cast.
///
/// # Errors
/// Returns `42804` naming the column type and the expression type.
pub(super) fn validate_assignment(
    expr: &Expr,
    types: &ResultTypes,
    expected: Option<&DataType>,
) -> Result<(), CassieError> {
    let Some(target) = descriptor(expected) else {
        return Ok(());
    };
    if !statically_typed(expr, types) {
        return Ok(());
    }
    let Some(source) = types.expression_type(expr) else {
        return Ok(());
    };
    let mismatch = match source.flattened() {
        DataType::Enum(other) => other.oid != target.oid,
        DataType::Text | DataType::Name | DataType::Varchar { .. } | DataType::Char { .. } => true,
        _ => false,
    };
    if !mismatch {
        return Ok(());
    }
    Err(DiagnosticError::new(
        "42804",
        format!(
            "column is of type {} but expression is of type {}",
            target.name,
            pg_name(&source)
        ),
    )
    .with_hint("You will need to rewrite or cast the expression.")
    .into())
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
