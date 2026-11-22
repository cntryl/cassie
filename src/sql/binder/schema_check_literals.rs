//! Types the literal bound of a simple CHECK to its column.
//!
//! The parser keeps the right-hand side as written. Row values are stored in
//! the column's canonical form, so the bound is converted to that form when
//! the constraint is created, as PostgreSQL does when it analyses the
//! expression. A bound the column cannot hold is rejected here: `22007`,
//! `22P02` or `22003` for text that is not a valid value of the type, `42883`
//! when no operator joins the column type and the literal type. A
//! `timestamptz` bound without an offset reads under the creating session's
//! `TimeZone`, which PostgreSQL also resolves once, at creation.

use super::super::BindingContext;
use crate::app::{CassieError, DiagnosticError};
use crate::catalog::{ConstraintCheck, ConstraintOperator, FieldConstraint};
use crate::types::extended::temporal::TemporalSession;
use crate::types::extended::{ExtendedFamily, ExtendedType};
use crate::types::{DataType, FieldSchema};
use serde_json::Value;

pub(super) fn bind(
    constraints: &mut [FieldConstraint],
    fields: &[FieldSchema],
    context: &BindingContext,
) -> Result<(), CassieError> {
    let session = context
        .temporal
        .as_deref()
        .unwrap_or_else(|| crate::types::extended::temporal::default_session());
    for check in constraints
        .iter_mut()
        .filter_map(|constraint| constraint.check.as_mut())
    {
        if check.value.is_null() {
            continue;
        }
        let Some(field) = fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(&check.field, &field.name)
        }) else {
            continue;
        };
        bind_check(check, field.data_type.flattened(), session)?;
    }
    Ok(())
}

fn bind_check(
    check: &mut ConstraintCheck,
    column: &DataType,
    session: &TemporalSession,
) -> Result<(), CassieError> {
    let literal = check.value.clone();
    let mismatch = || no_operator(column, &check.operator, &literal);
    let like = check.operator == ConstraintOperator::Like;
    // The literal as text; a number, a boolean or LIKE has no operator.
    let text = || literal.as_str().filter(|_| !like).ok_or_else(mismatch);
    // A number is comparable as it is; LIKE never applies.
    let number_or_text = || {
        (!like && (literal.is_number() || literal.is_string()))
            .then_some(())
            .ok_or_else(mismatch)
    };
    match column {
        DataType::Text | DataType::Name | DataType::Char { .. } | DataType::Varchar { .. } => {
            // Text accepts a string literal, and `LIKE` takes its pattern.
            if !literal.is_string() {
                return Err(mismatch());
            }
        }
        DataType::SmallInt | DataType::Int | DataType::BigInt => {
            number_or_text()?;
            if let Value::String(text) = &literal {
                check.value = text.trim().parse::<i64>().map(Value::from).map_err(|_| {
                    diagnostic(
                        "22P02",
                        format!("invalid input syntax for type integer: \"{text}\""),
                    )
                })?;
            }
        }
        DataType::Float => {
            number_or_text()?;
            if let Value::String(text) = &literal {
                check.value = float_bound(text)?;
            }
        }
        DataType::Boolean => {
            if !literal.is_boolean() || like {
                return Err(mismatch());
            }
        }
        DataType::Uuid => {
            let text = text()?;
            let parsed = uuid::Uuid::parse_str(text.trim()).map_err(|_| {
                diagnostic(
                    "22P02",
                    format!("invalid input syntax for type uuid: \"{text}\""),
                )
            })?;
            check.value = parsed.to_string().into();
        }
        DataType::Date => {
            let text = text()?;
            parse(ExtendedType::Date, text, session)?;
            check.value = stored_text(text, crate::types::temporal::canonical_date);
        }
        DataType::Time => {
            let text = text()?;
            parse(ExtendedType::Time { precision: None }, text, session)?;
            check.value = stored_text(text, crate::types::temporal::canonical_time);
        }
        DataType::Timestamp => {
            let text = text()?;
            parse(ExtendedType::Timestamp { precision: None }, text, session)?;
            check.value = stored_text(text, crate::types::temporal::canonical_timestamp);
        }
        DataType::Extended(kind) => match kind.family() {
            ExtendedFamily::Numeric => {
                number_or_text()?;
                if let Value::String(text) = &literal {
                    parse(*kind, text, session)?;
                }
            }
            ExtendedFamily::Temporal | ExtendedFamily::Timespan => {
                check.value = parse(*kind, text()?, session)?;
            }
            _ => {
                parse(*kind, text()?, session)?;
            }
        },
        _ => {}
    }
    Ok(())
}

/// Reads `text` as `kind` and returns the JSON carrier rows of that type use.
fn parse(kind: ExtendedType, text: &str, session: &TemporalSession) -> Result<Value, CassieError> {
    kind.parse_text_in(text, session)
        .map(|typed| typed.to_json())
        .map_err(|error| diagnostic(error.sqlstate, error.message))
}

/// The canonical stored text of a date, time or timestamp that already parsed.
fn stored_text(text: &str, canonicalize: fn(&str) -> Result<String, String>) -> Value {
    canonicalize(text).map_or_else(|_| text.into(), Into::into)
}

/// A float bound written as text. Finite values become JSON numbers; `NaN` and
/// the infinities stay text, which JSON numbers cannot carry.
fn float_bound(text: &str) -> Result<Value, CassieError> {
    use crate::types::float_input::{parse, FloatInputError};
    let value = parse(text).map_err(|error| {
        let message = match error {
            FloatInputError::Syntax => {
                format!("invalid input syntax for type double precision: \"{text}\"")
            }
            FloatInputError::Range => {
                format!("\"{text}\" is out of range for type double precision")
            }
        };
        diagnostic(error.sql_state(), message)
    })?;
    Ok(if value.is_nan() {
        "NaN".into()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        value.into()
    })
}

fn diagnostic(sql_state: &'static str, message: String) -> CassieError {
    DiagnosticError::new(sql_state, message).into()
}

fn no_operator(column: &DataType, operator: &ConstraintOperator, literal: &Value) -> CassieError {
    let right = match literal {
        Value::Bool(_) => "boolean",
        Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        Value::Number(_) => "numeric",
        _ => "unknown",
    };
    diagnostic(
        "42883",
        format!(
            "operator does not exist: {} {} {right}",
            pg_type_name(column),
            super::enum_checks::operator_symbol(operator)
        ),
    )
}

fn pg_type_name(column: &DataType) -> String {
    match column {
        DataType::Int => "integer".to_string(),
        DataType::Float => "double precision".to_string(),
        DataType::Varchar { .. } => "character varying".to_string(),
        DataType::Char { .. } => "character".to_string(),
        other => other.type_name(),
    }
}
