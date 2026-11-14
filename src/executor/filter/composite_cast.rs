//! Casts into composite types: text literals and anonymous `ROW(...)` records.

use super::{cast_scalar, scalar_from_value, EvalContext, Expr, RowAccess, ScalarValue};
use crate::app::{CassieError, DiagnosticError};
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{RowComparison, RowComparisonOp, ROW_CONSTRUCTOR_FUNCTION};
use crate::types::composite::{parse_record_text, RecordInputError};
use crate::types::typed::TypedValue;
use crate::types::{CompositeRef, DataType, RecordValue, Value};

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

/// How strictly a field's `char(n)`/`varchar(n)` length is applied.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Coercion {
    /// An explicit `ROW(...)::type` cast discards any excess text.
    Explicit,
    /// Column assignment and record text input discard only excess blanks.
    Assignment,
}

pub(super) fn cast(
    value: &ScalarValue,
    descriptor: &CompositeRef,
    controls: Option<&QueryExecutionControls>,
) -> Result<ScalarValue, QueryError> {
    cast_with(value, descriptor, controls, Coercion::Explicit)
}

/// Assignment of `value` to a column of the composite type `descriptor`: a
/// too-long `char(n)`/`varchar(n)` field is SQLSTATE `22001`, as for a column.
///
/// # Errors
///
/// Returns the cast error of the value or of one of its fields.
pub(crate) fn assign_record(value: &Value, descriptor: &CompositeRef) -> Result<Value, QueryError> {
    cast_with(
        &scalar_from_value(value),
        descriptor,
        None,
        Coercion::Assignment,
    )
    .map(ScalarValue::into_value)
}

fn cast_with(
    value: &ScalarValue,
    descriptor: &CompositeRef,
    controls: Option<&QueryExecutionControls>,
    coercion: Coercion,
) -> Result<ScalarValue, QueryError> {
    match value {
        ScalarValue::Typed(TypedValue::Record(record)) => {
            cast_record(record, descriptor, controls, coercion).map(typed)
        }
        ScalarValue::Str(text) => parse_record_text(text, descriptor, |data_type, part| {
            cast_field(
                &ScalarValue::Str(part.to_owned()),
                data_type,
                controls,
                Coercion::Assignment,
            )
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

/// Casts one field and returns the carrier a column of that type holds:
/// `json`/`jsonb` fields keep a parsed document, so operators and storage see
/// the document rather than its text.
fn cast_field(
    value: &ScalarValue,
    data_type: &DataType,
    controls: Option<&QueryExecutionControls>,
    coercion: Coercion,
) -> Result<Value, QueryError> {
    if let (Coercion::Assignment, DataType::Composite(nested), false) = (
        coercion,
        data_type.flattened(),
        matches!(value, ScalarValue::Null),
    ) {
        return cast_with(value, nested, controls, coercion).map(ScalarValue::into_value);
    }
    let bounded = match coercion {
        Coercion::Assignment => assigned_length(value, data_type)?,
        Coercion::Explicit => None,
    };
    let cast = cast_scalar(bounded.as_ref().unwrap_or(value), data_type, controls)?;
    Ok(match cast {
        ScalarValue::Json(text) | ScalarValue::Str(text) if data_type.flattened().is_json() => {
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(document) => Value::Json(document),
                Err(_) => Value::String(text),
            }
        }
        other => other.into_value(),
    })
}

/// The assignment form of a text value for a `char(n)`/`varchar(n)` field:
/// excess blanks are discarded and any other excess text is SQLSTATE `22001`.
/// `None` leaves the value to the ordinary cast.
fn assigned_length(
    value: &ScalarValue,
    data_type: &DataType,
) -> Result<Option<ScalarValue>, QueryError> {
    let (width, padded) = match data_type.flattened() {
        DataType::Char {
            length: Some(length),
        } => (*length, true),
        DataType::Varchar {
            length: Some(length),
        } => (*length, false),
        _ => return Ok(None),
    };
    let ScalarValue::Str(text) = value else {
        return Ok(None);
    };
    let plan = crate::types::char_text::conversion_plan(text, Some(width), padded, false)
        .map_err(CassieError::TypeInput)?;
    Ok(Some(ScalarValue::Str(text[..plan.prefix_bytes].to_owned())))
}

/// `record::text` renders its fields in the session's `TimeZone`, `DateStyle`
/// and `IntervalStyle`, exactly as the record itself is sent. `None` for any
/// other value.
pub(super) fn session_text(
    value: &ScalarValue,
    controls: Option<&QueryExecutionControls>,
) -> Option<String> {
    let ScalarValue::Typed(record @ TypedValue::Record(_)) = value else {
        return None;
    };
    Some(record.text_in(&super::temporal::session_of(controls)))
}

/// `ROW(...) IN (ROW(...), ...)` compares two constructors field by field, as
/// `ROW(...) = ROW(...)` does: a NULL field leaves an otherwise equal pair
/// unknown instead of unequal. `None` unless both sides are constructors.
pub(super) fn constructor_equality<R: RowAccess + ?Sized>(
    row: &R,
    left: &Expr,
    right: &Expr,
    context: EvalContext<'_>,
) -> Result<Option<ScalarValue>, QueryError> {
    let (Expr::Function(left), Expr::Function(right)) = (left, right) else {
        return Ok(None);
    };
    if !left.name.eq_ignore_ascii_case(ROW_CONSTRUCTOR_FUNCTION)
        || !right.name.eq_ignore_ascii_case(ROW_CONSTRUCTOR_FUNCTION)
    {
        return Ok(None);
    }
    let comparison = RowComparison {
        left: left.args.clone(),
        op: RowComparisonOp::Eq,
        right: right.args.clone(),
    };
    super::row_predicates::evaluate(row, &comparison, context).map(Some)
}

fn typed(record: RecordValue) -> ScalarValue {
    ScalarValue::Typed(TypedValue::Record(record))
}

/// Field-wise cast of an anonymous record, or identity for the same type.
fn cast_record(
    record: &RecordValue,
    target: &CompositeRef,
    controls: Option<&QueryExecutionControls>,
    coercion: Coercion,
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
        fields.push(cast_field(
            &scalar_from_value(value),
            &field.data_type,
            controls,
            coercion,
        )?);
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

/// Parses PostgreSQL record text into a record of `descriptor` (the wire layer
/// sends rendered record text in binary `record_send` layout).
///
/// # Errors
///
/// Returns the input or field conversion error of a malformed literal.
pub(crate) fn record_from_text(
    text: &str,
    descriptor: &CompositeRef,
) -> Result<crate::types::Value, QueryError> {
    cast(&ScalarValue::Str(text.to_owned()), descriptor, None).map(|value| value.to_value())
}
