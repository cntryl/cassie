//! Private SQL-to-JSON conversion shared by scalar builders and collections.
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul, serialized_json_bytes};
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::typed::TypedValue;
use crate::types::{DataType, Value};

pub(super) fn bound(
    value: &Value,
    kind: Option<&DataType>,
    binary: bool,
) -> Result<usize, CassieError> {
    match value {
        Value::String(text) if document_type(kind) => {
            if binary {
                crate::types::jsonb::input_text_len(text).map_err(CassieError::TypeInput)
            } else {
                Ok(text.len())
            }
        }
        Value::String(text) => add(mul(text.len(), 6)?, 2),
        Value::Json(value) if binary => {
            crate::types::jsonb::text_len(value).map_err(CassieError::TypeInput)
        }
        Value::Json(value) => serialized_json_bytes(value),
        Value::Typed(TypedValue::Numeric(crate::types::extended::numeric::Numeric::Finite {
            mantissa,
            scale,
        })) => add(
            add(
                usize::try_from(mantissa.bits())
                    .unwrap_or(usize::MAX)
                    .max(1),
                *scale as usize,
            )?,
            5,
        ),
        Value::Typed(value) => add(mul(value.heap_bytes(), 8)?, 128),
        Value::Array(array) => array.elements().iter().try_fold(
            add(mul(array.elements().len(), 16)?, 256)?,
            |bytes, element| add(bytes, mul(bound(element, None, binary)?, 6)?),
        ),
        Value::Vector(value) => add(mul(value.values.len(), 64)?, 2),
        Value::Null | Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => Ok(64),
    }
}

fn document_type(kind: Option<&DataType>) -> bool {
    matches!(
        kind,
        Some(DataType::Json | DataType::Jsonb | DataType::Array(_))
    )
}

pub(super) fn encode(
    value: &Value,
    kind: Option<&DataType>,
    controls: Option<&QueryExecutionControls>,
) -> Result<String, QueryError> {
    match value {
        Value::String(text) if document_type(kind) => Ok(text.clone()),
        Value::String(text) if kind == Some(&DataType::Timestamp) => {
            let text = crate::types::temporal::timestamp_text(text).map_err(QueryError::General)?;
            quote(&text.replace(' ', "T"))
        }
        Value::String(text) => quote(text),
        Value::Json(value) => Ok(value.to_string()),
        Value::Array(value) => Ok(value.to_nested_json().to_string()),
        Value::Null => Ok("null".into()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Int64(value) => Ok(value.to_string()),
        Value::Float64(value) => floating(*value),
        Value::Typed(TypedValue::Numeric(value)) => {
            let text = value.to_string();
            if matches!(
                value,
                crate::types::extended::numeric::Numeric::Finite { .. }
            ) {
                Ok(text)
            } else {
                quote(&text)
            }
        }
        Value::Typed(TypedValue::Real(value)) => {
            let text = crate::types::extended::real::format_text(*value);
            if value.is_finite() {
                Ok(text)
            } else {
                quote(&text)
            }
        }
        Value::Typed(value) => quote(&typed_value_text(value, controls)),
        Value::Vector(value) => {
            let text = serde_json::to_string(&value.values)
                .map_err(|error| QueryError::General(error.to_string()))?;
            quote(&text)
        }
    }
}

fn typed_value_text(value: &TypedValue, controls: Option<&QueryExecutionControls>) -> String {
    let session = crate::executor::filter::temporal::session_of(controls);
    match value {
        TypedValue::Timestamp(value) => {
            crate::types::extended::temporal::encode::format_json_timestamp(*value, &session, false)
        }
        TypedValue::TimestampTz(value) => {
            crate::types::extended::temporal::encode::format_json_timestamp(*value, &session, true)
        }
        TypedValue::Date(_) => {
            let mut iso = (*session).clone();
            iso.date_output = crate::types::extended::temporal::session::DateOutput::Iso;
            value.text_in(&iso)
        }
        _ => value.text_in(&session),
    }
}

pub(super) fn key(
    value: &Value,
    kind: Option<&DataType>,
    controls: Option<&QueryExecutionControls>,
) -> Result<String, QueryError> {
    if matches!(value, Value::Null | Value::Json(_))
        || document_type(kind)
        || matches!(value, Value::Vector(_))
    {
        return Err(CassieError::InvalidParameterValue(
            "JSON object keys must be non-NULL scalar values".into(),
        )
        .into());
    }
    let text = scalar_key_text(value, controls)
        .expect("document and non-scalar JSON object keys are rejected above");
    quote(&text)
}

pub(super) fn scalar_key_text(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Option<String> {
    Some(match value {
        Value::String(text) => text.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Int64(value) => value.to_string(),
        Value::Float64(value) => crate::types::float_text::format(*value),
        Value::Typed(value) => {
            value.text_in(&crate::executor::filter::temporal::session_of(controls))
        }
        Value::Null | Value::Json(_) | Value::Array(_) | Value::Vector(_) => return None,
    })
}

pub(super) fn workspace_bytes(bytes: usize) -> Result<usize, CassieError> {
    add(
        crate::runtime::accounted::json::serialized_decode_bytes(bytes)?,
        mul(bytes.max(8), 8)?,
    )
}

pub(super) fn reserve(
    controls: Option<&QueryExecutionControls>,
    bytes: usize,
) -> Result<Option<QueryMemoryReservation>, QueryError> {
    check(controls)?;
    let bytes = workspace_bytes(bytes)?;
    controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()
        .map_err(Into::into)
}

pub(super) fn finish(
    document: String,
    binary: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    check(controls)?;
    let document = if binary {
        crate::types::jsonb::canonical(&document).map_err(CassieError::TypeInput)?
    } else {
        document
    };
    check(controls)?;
    Ok(Value::String(document))
}

pub(super) fn check(controls: Option<&QueryExecutionControls>) -> Result<(), QueryError> {
    controls.map_or(Ok(()), |controls| {
        controls.check_interruption().map_err(Into::into)
    })
}

fn floating(value: f64) -> Result<String, QueryError> {
    let text = crate::types::float_text::format(value);
    if value.is_finite() {
        Ok(text)
    } else {
        quote(&text)
    }
}

fn quote(text: &str) -> Result<String, QueryError> {
    serde_json::to_string(text).map_err(|error| QueryError::General(error.to_string()))
}

/// Temporary expression-type lookup and the retained argument type descriptor.
pub(super) fn typing_bytes<R: crate::executor::batch::RowAccess + ?Sized>(
    row: &R,
    expr: &crate::sql::ast::Expr,
    functions: &std::collections::HashMap<String, crate::catalog::FunctionMeta>,
) -> Result<usize, CassieError> {
    use crate::sql::ast::Expr;
    let mut bytes = 4 * std::mem::size_of::<DataType>();
    let descriptor = match expr {
        Expr::Column(name) => {
            bytes = add(bytes, mul(name.len(), 4)?)?;
            row.column_type(name)
        }
        Expr::Cast { data_type, .. } => Some(data_type),
        Expr::Subquery(subquery) => subquery.data_type.as_ref(),
        Expr::Function(function) => {
            bytes = add(bytes, mul(function.name.len(), 4)?)?;
            functions
                .get(&function.name)
                .map(|metadata| &metadata.return_type)
        }
        _ => None,
    };
    if let Some(kind) = descriptor {
        bytes = add(
            bytes,
            mul(
                crate::executor::retained_memory::data_type_clone_bytes(kind)?,
                4,
            )?,
        )?;
    }
    expr.try_visit_children(|child| {
        bytes = add(bytes, typing_bytes(row, child, functions)?)?;
        Ok::<_, CassieError>(())
    })?;
    Ok(bytes)
}

#[cfg(test)]
mod numeric_boundary_tests;
