//! Definition-time validation of domain CHECK expressions and defaults (#828).
//!
//! PostgreSQL rejects a CHECK that is not boolean (`42804`) or that calls an
//! unknown function (`42883`) when the constraint is declared, instead of
//! leaving a domain no value can ever satisfy.

use super::QueryError;
use crate::app::{CassieError, DiagnosticError};
use crate::sql::ast::{BinaryOp, Expr};
use crate::types::domain::DOMAIN_VALUE_COLUMN;
use crate::types::{DataType, DomainCheck, Value};

const UNKNOWN_FUNCTION_PREFIX: &str = "unsupported function '";

fn diagnostic(state: &'static str, message: String) -> QueryError {
    CassieError::from(DiagnosticError::new(state, message)).into()
}

/// PostgreSQL's spelling of `data_type` in a type mismatch message.
fn type_spelling(data_type: &DataType) -> String {
    match data_type {
        DataType::Int => "integer".to_owned(),
        DataType::Float => "double precision".to_owned(),
        other => other.type_name(),
    }
}

/// The type of `expression` when its outermost shape can never be boolean:
/// the bare `VALUE` of a non-boolean base, arithmetic, concatenation, a
/// numeric literal or a cast to a non-boolean type.
fn evident_non_boolean_type(expression: &Expr, base: &DataType) -> Option<DataType> {
    let base = base.flattened();
    let non_boolean = |data_type: &DataType| !matches!(data_type, DataType::Boolean);
    match expression {
        Expr::Column(name) if name.eq_ignore_ascii_case(DOMAIN_VALUE_COLUMN) => {
            non_boolean(base).then(|| base.clone())
        }
        Expr::Cast { data_type, .. } => {
            let target = data_type.flattened();
            non_boolean(target).then(|| target.clone())
        }
        Expr::IntegerLiteral(value) => Some(crate::sql::binder::integer_literal_type(*value)),
        Expr::NumberLiteral(_) => Some(DataType::Float),
        Expr::Binary {
            op:
                BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::Modulo
                | BinaryOp::Concat,
            ..
        } => Some(if non_boolean(base) {
            base.clone()
        } else {
            DataType::Text
        }),
        _ => None,
    }
}

/// Rejects a CHECK expression PostgreSQL rejects at declaration.
///
/// # Errors
///
/// Returns `42804` for an evidently non-boolean expression and `42883` when
/// the expression calls a function that does not exist.
pub(super) fn validate(expression: &Expr, sql: &str, base: &DataType) -> Result<(), QueryError> {
    if let Some(found) = evident_non_boolean_type(expression, base) {
        return Err(diagnostic(
            "42804",
            format!(
                "argument of CHECK must be type boolean, not type {}",
                type_spelling(&found)
            ),
        ));
    }
    // A CHECK evaluates for NULL as well, so one trial evaluation finds a
    // function the executor cannot resolve. Any other outcome is left to the
    // values the constraint will actually see.
    let trial = DomainCheck {
        name: String::new(),
        sql: sql.to_owned(),
        validated: true,
    };
    let Err(error) = crate::executor::filter::evaluate_check(&trial, &Value::Null) else {
        return Ok(());
    };
    let message = error.to_string();
    let Some(start) = message.find(UNKNOWN_FUNCTION_PREFIX) else {
        return Ok(());
    };
    let function = message[start + UNKNOWN_FUNCTION_PREFIX.len()..]
        .split('\'')
        .next()
        .unwrap_or_default();
    Err(diagnostic(
        "42883",
        format!(
            "function {function}({}) does not exist",
            type_spelling(base.flattened())
        ),
    ))
}

/// The text of a DEFAULT that is a plain exact numeric literal for a
/// `numeric` base, so the stored default keeps the written scale.
pub(super) fn exact_numeric_default(base: &DataType, sql: &str) -> Option<String> {
    if !matches!(
        base.flattened(),
        DataType::Extended(crate::types::extended::ExtendedType::Numeric(_))
    ) {
        return None;
    }
    let text = sql.trim();
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    let plain = !digits.is_empty()
        && digits
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.');
    (plain && crate::types::extended::numeric::parse_text(text).is_ok()).then(|| text.to_owned())
}
