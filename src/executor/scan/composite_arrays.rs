//! Reads stored arrays of named composite types back into runtime values (#1268).
//!
//! Each element is stored as its record document; the fields and their types
//! come from the column's descriptor. A document the descriptor does not
//! accept is data corruption (`XX001`).
use crate::app::{CassieError, DiagnosticError};
use crate::types::composite::CompositeRef;
use crate::types::{DataType, SqlArray, Value};

/// The record descriptor of an array-of-composite column, at any depth.
pub(super) fn element_descriptor(data_type: &DataType) -> Option<&CompositeRef> {
    let DataType::Array(mut element) = data_type else {
        return None;
    };
    while let DataType::Array(inner) = &**element {
        element = inner;
    }
    match &**element {
        DataType::Composite(descriptor) => Some(descriptor),
        _ => None,
    }
}

fn corrupt(detail: &str) -> CassieError {
    DiagnosticError::new("XX001", format!("corrupt stored value: {detail}")).into()
}

/// Restores a stored composite-array document for a column whose elements are
/// `descriptor` records.
///
/// # Errors
/// Returns `XX001` when the array or one of its records is malformed.
pub(super) fn from_storage(
    document: &serde_json::Value,
    descriptor: &CompositeRef,
) -> Result<Value, CassieError> {
    if document.is_null() {
        return Ok(Value::Null);
    }
    let element_type = DataType::Composite(descriptor.clone());
    let stored = if document
        .as_object()
        .is_some_and(|object| object.contains_key("$cassie_array_v1"))
    {
        SqlArray::from_storage_json(document).map_err(|detail| corrupt(&detail))?
    } else {
        SqlArray::from_nested_json_typed(document, &element_type)
            .map_err(|detail| corrupt(&detail))?
    };
    typed_records(&stored, descriptor)
}

/// `array` with every element as a typed `descriptor` record. A stored record
/// document and record text are both converted; SQL NULL stays NULL.
///
/// # Errors
/// Returns `XX001` for a malformed document and the input error for text that
/// is not a record of this type.
pub(crate) fn typed_records(
    array: &SqlArray,
    descriptor: &CompositeRef,
) -> Result<Value, CassieError> {
    let element_type = DataType::Composite(descriptor.clone());
    let elements = array
        .elements()
        .iter()
        .map(|element| match element {
            Value::Json(record) => super::try_json_to_typed_value(record, &element_type),
            Value::String(text) => crate::executor::filter::record_from_text(text, descriptor)
                .map_err(|error| corrupt(&error.to_string())),
            other => Ok(other.clone()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    SqlArray::new(array.dimensions().to_vec(), elements)
        .map(Value::Array)
        .map_err(|detail| corrupt(&detail))
}

/// Parses PostgreSQL array text whose elements are `descriptor` records.
///
/// # Errors
/// Returns an error when the text is not an array of such records.
pub(crate) fn from_text(text: &str, descriptor: &CompositeRef) -> Result<Value, CassieError> {
    let element_type = DataType::Composite(descriptor.clone());
    let parsed = crate::types::array::parse_sql_text_array(text, &element_type)
        .map_err(|error| CassieError::ArrayInput(error.to_string()))?;
    let array = parsed
        .into_sql_array(&element_type)
        .map_err(CassieError::ArrayInput)?;
    typed_records(&array, descriptor)
}
