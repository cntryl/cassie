//! Scalar JSON construction and pre-allocation bounds.
use super::{CassieError, DataType, QueryError, Value};
use crate::executor::retained_memory::{add, mul, serialized_json_bytes};
use crate::runtime::QueryExecutionControls;
use crate::types::typed::TypedValue;

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
        Value::Array(_) => add(
            crate::executor::retained_memory::value_clone_bytes(value)?,
            128,
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
        Value::Typed(value) => {
            let mut text = value.text_in(&super::super::super::temporal::session_of(controls));
            if matches!(value, TypedValue::Timestamp(_) | TypedValue::TimestampTz(_)) {
                text = text.replacen(' ', "T", 1);
            }
            quote(&text)
        }
        Value::Vector(value) => {
            let text = serde_json::to_string(&value.values)
                .map_err(|error| QueryError::General(error.to_string()))?;
            quote(&text)
        }
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
    let text = match value {
        Value::String(text) => text.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Int64(value) => value.to_string(),
        Value::Float64(value) => crate::types::float_text::format(*value),
        Value::Typed(value) => value.text_in(&super::super::super::temporal::session_of(controls)),
        _ => unreachable!("non-scalar keys rejected"),
    };
    quote(&text)
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
