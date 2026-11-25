//! Reads stored named enum values back into runtime values (#827).
//!
//! A stored enum is `{"$cassie_enum_v1": member_id}`; the label and the order
//! come from the column's descriptor. A member id the descriptor does not know
//! is data corruption (`XX001`).
use crate::app::{CassieError, DiagnosticError};
use crate::types::typed::TypedValue;
use crate::types::{DataType, EnumRef, EnumValue, SqlArray, Value};

/// Whether `data_type` is an enum or an array of enums.
pub(super) const fn is_enum_column(data_type: &DataType) -> bool {
    match data_type {
        DataType::Enum(_) => true,
        DataType::Array(element) => matches!(**element, DataType::Enum(_)),
        _ => false,
    }
}

fn corrupt(detail: &str) -> CassieError {
    DiagnosticError::new("XX001", format!("corrupt stored value: {detail}")).into()
}

fn member(descriptor: &EnumRef, document: &serde_json::Value) -> Result<Value, CassieError> {
    let member_id =
        EnumValue::storage_member_id(document).ok_or_else(|| corrupt("not an enum document"))?;
    EnumValue::new(descriptor.clone(), member_id)
        .map(|value| Value::Typed(TypedValue::Enum(value)))
        .ok_or_else(|| {
            corrupt(&format!(
                "unknown member id {member_id} for enum {}",
                descriptor.name
            ))
        })
}

fn array(descriptor: &EnumRef, document: &serde_json::Value) -> Result<Value, CassieError> {
    let stored = if document
        .as_object()
        .is_some_and(|object| object.contains_key("$cassie_array_v1"))
    {
        SqlArray::from_storage_json(document).map_err(|detail| corrupt(&detail))?
    } else {
        SqlArray::from_nested_json_typed(document, &DataType::Enum(descriptor.clone()))
            .map_err(|detail| corrupt(&detail))?
    };
    let elements = stored
        .elements()
        .iter()
        .map(|element| match element {
            Value::Json(document) => member(descriptor, document),
            other => Ok(other.clone()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    SqlArray::new(stored.dimensions().to_vec(), elements)
        .map(Value::Array)
        .map_err(|detail| corrupt(&detail))
}

/// Restores a stored enum or enum-array document for a column of `data_type`.
///
/// # Errors
/// Returns `XX001` when the document contradicts the column's descriptor.
pub(super) fn from_storage(
    value: &serde_json::Value,
    data_type: &DataType,
) -> Result<Value, CassieError> {
    if value.is_null() {
        return Ok(Value::Null);
    }
    match data_type {
        DataType::Enum(descriptor) => member(descriptor, value),
        DataType::Array(element) => match element.as_ref() {
            DataType::Enum(descriptor) => array(descriptor, value),
            _ => Err(corrupt("not an enum array column")),
        },
        _ => Err(corrupt("not an enum column")),
    }
}

#[cfg(test)]
#[path = "enum_values_tests.rs"]
mod tests;
