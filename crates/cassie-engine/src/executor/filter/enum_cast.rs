//! Casts into and out of named enum types (#827).
//!
//! `unknown`/`text`/`varchar`/`name` -> enum parses a label (`22P02`, or
//! `55P04` for a label added in the open transaction); enum -> `text`-family
//! yields the label; enum -> the same enum is the identity. Every other cast
//! is `42846`, as in PostgreSQL.

use super::{cast_scalar, ScalarValue};
use crate::app::{CassieError, DiagnosticError};
use crate::executor::QueryError;
use crate::types::typed::TypedValue;
use crate::types::{DataType, EnumInputError, EnumRef, EnumValue, Value};

fn query_error(error: DiagnosticError) -> QueryError {
    QueryError::Cassie(CassieError::from(error))
}

/// Maps an `enum_in` failure to its PostgreSQL diagnostic.
pub(crate) fn input_diagnostic(error: &EnumInputError) -> DiagnosticError {
    let diagnostic = DiagnosticError::new(error.sqlstate(), error.message());
    match error.hint() {
        Some(hint) => diagnostic.with_hint(hint),
        None => diagnostic,
    }
}

/// `label::enum`: the value of `descriptor` named `label`.
///
/// # Errors
/// Returns `22P02` for an unknown label and `55P04` for a label added by the
/// open transaction to a type that existed before it.
pub(crate) fn cast_text_to_enum(descriptor: &EnumRef, label: &str) -> Result<Value, CassieError> {
    EnumValue::parse_label(descriptor, label)
        .map(|value| Value::Typed(TypedValue::Enum(value)))
        .map_err(|error| CassieError::from(input_diagnostic(&error)))
}

/// PostgreSQL's spelling of a type in `cannot cast type x to y`.
fn pg_type_name(data_type: &DataType) -> String {
    match data_type {
        DataType::Int => "integer".to_owned(),
        DataType::Float => "double precision".to_owned(),
        other => other.type_name(),
    }
}

fn source_label(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Bool(_) => "boolean".to_owned(),
        ScalarValue::Int(_) => "bigint".to_owned(),
        ScalarValue::Float(_) => "double precision".to_owned(),
        ScalarValue::Json(_) => "jsonb".to_owned(),
        ScalarValue::Typed(typed) => typed.data_type().type_name(),
        ScalarValue::Str(_) | ScalarValue::Array(_) | ScalarValue::Null => "unknown".to_owned(),
    }
}

fn cannot_cast(source: &str, target: &str) -> QueryError {
    query_error(DiagnosticError::new(
        "42846",
        format!("cannot cast type {source} to {target}"),
    ))
}

pub(super) fn into_enum(
    value: &ScalarValue,
    descriptor: &EnumRef,
) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Str(label) => cast_text_to_enum(descriptor, label)
            .map(|value| scalar_enum(&value))
            .map_err(QueryError::Cassie),
        ScalarValue::Typed(TypedValue::Enum(source))
            if source.descriptor().oid == descriptor.oid =>
        {
            Ok(ScalarValue::Typed(TypedValue::Enum(
                // The target descriptor is the current revision of the same type.
                EnumValue::new(descriptor.clone(), source.member_id())
                    .unwrap_or_else(|| source.clone()),
            )))
        }
        other => Err(cannot_cast(&source_label(other), &descriptor.name)),
    }
}

fn scalar_enum(value: &Value) -> ScalarValue {
    match value {
        Value::Typed(typed) => ScalarValue::Typed(typed.clone()),
        _ => ScalarValue::Null,
    }
}

fn out_of_enum(
    source: &EnumValue,
    target: &DataType,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<ScalarValue, QueryError>> {
    match target {
        // Text and domains over it are handled by the generic cast.
        DataType::Text | DataType::Domain(_) | DataType::Enum(_) => None,
        DataType::Name | DataType::Varchar { .. } | DataType::Char { .. } => Some(cast_scalar(
            &ScalarValue::Str(source.label().to_owned()),
            target,
            controls,
        )),
        other => Some(Err(cannot_cast(
            &source.descriptor().name,
            &pg_type_name(other),
        ))),
    }
}

/// The cast out of an enum value: `None` leaves the cast to the generic
/// implementation (a non-enum value, or a text or domain target).
pub(super) fn cast_from_enum(
    value: &ScalarValue,
    target: &DataType,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Option<Result<ScalarValue, QueryError>> {
    match value {
        ScalarValue::Typed(TypedValue::Enum(source)) => out_of_enum(source, target, controls),
        _ => None,
    }
}

#[cfg(test)]
#[path = "enum_cast_tests.rs"]
mod tests;
