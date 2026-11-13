//! Casts into composite types: text literals and anonymous `ROW(...)` records.

use super::{cast_scalar, scalar_from_value, ScalarValue};
use crate::app::{CassieError, DiagnosticError};
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;
use crate::types::composite::{parse_record_text, RecordInputError};
use crate::types::typed::TypedValue;
use crate::types::{CompositeRef, RecordValue, Value};

enum CastFailure {
    Input(RecordInputError),
    Query(QueryError),
}

impl From<RecordInputError> for CastFailure {
    fn from(error: RecordInputError) -> Self {
        Self::Input(error)
    }
}

fn diagnostic(state: &'static str, message: String, detail: Option<String>) -> QueryError {
    let mut error = DiagnosticError::new(state, message);
    if let Some(detail) = detail {
        error = error.with_detail(detail);
    }
    QueryError::Cassie(CassieError::from(error))
}

fn input_error(error: RecordInputError) -> QueryError {
    diagnostic(error.sqlstate, error.message, error.detail)
}

pub(super) fn cast(
    value: &ScalarValue,
    descriptor: &CompositeRef,
    controls: Option<&QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Typed(TypedValue::Record(record)) => {
            cast_record(record, descriptor, controls).map(typed)
        }
        ScalarValue::Str(text) => parse_record_text(text, descriptor, |data_type, part| {
            cast_scalar(&ScalarValue::Str(part.to_owned()), data_type, controls)
                .map(|value| value.to_value())
                .map_err(CastFailure::Query)
        })
        .map(typed)
        .map_err(|failure| match failure {
            CastFailure::Input(error) => input_error(error),
            CastFailure::Query(error) => error,
        }),
        other => Err(diagnostic(
            "42846",
            format!(
                "cannot cast type {} to {}",
                scalar_label(other),
                descriptor.name
            ),
            None,
        )),
    }
}

fn typed(record: RecordValue) -> ScalarValue {
    ScalarValue::Typed(TypedValue::Record(record))
}

/// Field-wise cast of an anonymous record, or identity for the same type.
fn cast_record(
    record: &RecordValue,
    target: &CompositeRef,
    controls: Option<&QueryExecutionControls>,
) -> Result<RecordValue, QueryError> {
    let source = record.descriptor();
    if !source.is_anonymous() {
        if source.oid == target.oid {
            return Ok(record.clone());
        }
        return Err(diagnostic(
            "42846",
            format!("cannot cast type {} to {}", source.name, target.name),
            None,
        ));
    }
    if record.fields().len() != target.fields.len() {
        let detail = if record.fields().len() < target.fields.len() {
            "Input has too few columns."
        } else {
            "Input has too many columns."
        };
        return Err(diagnostic(
            "42846",
            format!("cannot cast type record to {}", target.name),
            Some(detail.to_owned()),
        ));
    }
    let mut fields = Vec::with_capacity(target.fields.len());
    for (value, field) in record.fields().iter().zip(&target.fields) {
        // A NULL still meets a NOT NULL domain field.
        fields.push(cast_scalar(&scalar_from_value(value), &field.data_type, controls)?.to_value());
    }
    RecordValue::new(target.clone(), fields).map_err(|message| diagnostic("XX000", message, None))
}

fn scalar_label(value: &ScalarValue) -> &'static str {
    match value {
        ScalarValue::Bool(_) => "boolean",
        ScalarValue::Int(_) => "bigint",
        ScalarValue::Float(_) => "double precision",
        ScalarValue::Json(_) => "jsonb",
        _ => "unknown",
    }
}

/// `IS [NOT] NULL`. A record is NULL when every field is NULL and NOT NULL
/// only when no field is NULL, so `IS NOT NULL` is not the negation of
/// `IS NULL` for a partly-NULL record.
pub(super) fn null_test(value: &ScalarValue, negated: bool) -> bool {
    let (is_null, is_not_null) = match value {
        ScalarValue::Null => (true, false),
        ScalarValue::Typed(crate::types::typed::TypedValue::Record(record)) => {
            (record.all_fields_null(), record.no_field_null())
        }
        _ => (false, true),
    };
    if negated {
        is_not_null
    } else {
        is_null
    }
}
