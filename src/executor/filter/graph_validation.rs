//! Hook of the shared `cassie_graph` validator into record construction (#1268).
use crate::app::{CassieError, DiagnosticError};
use crate::executor::QueryError;
use crate::types::RecordValue;

/// Validates `record` when it is a reserved graph type (SQLSTATE `22023`);
/// every other record passes untouched.
///
/// # Errors
/// Returns `22023` with the violated invariant.
pub(crate) fn validate(record: &RecordValue) -> Result<(), QueryError> {
    crate::types::graph::validate_record(record).map_err(|error| {
        QueryError::Cassie(CassieError::from(DiagnosticError::new(
            "22023",
            error.message().to_owned(),
        )))
    })
}

/// Whether a value of `data_type` can contain a reserved graph type, at any
/// depth of composite fields and arrays.
fn mentions_reserved(data_type: &crate::types::DataType) -> bool {
    use crate::types::DataType;
    match data_type {
        DataType::Composite(descriptor) => {
            crate::types::graph::GraphTypeKind::of(descriptor).is_some()
                || descriptor
                    .fields
                    .iter()
                    .any(|field| mentions_reserved(&field.data_type))
        }
        DataType::Array(inner) => mentions_reserved(inner),
        _ => false,
    }
}

/// Validates every reserved-type record inside a decoded value.
fn validate_nested(value: &crate::types::Value) -> Result<(), QueryError> {
    use crate::types::{typed::TypedValue, Value};
    match value {
        Value::Typed(TypedValue::Record(record)) => {
            validate(record)?;
            record.fields().iter().try_for_each(validate_nested)
        }
        Value::Array(array) => array.elements().iter().try_for_each(validate_nested),
        _ => Ok(()),
    }
}

/// Validates the reserved-type values of a document that a write path stores
/// directly (REST documents, projector replay), where no SQL cast has run.
/// Fields that cannot contain a reserved graph type are not decoded.
///
/// # Errors
/// Returns `22023` with the violated invariant, or the decode error of a
/// value that does not match its column type.
pub(crate) fn validate_stored_fields(
    schema: &crate::types::Schema,
    document: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), CassieError> {
    for field in &schema.fields {
        let Some(value) = document.get(&field.name).filter(|value| !value.is_null()) else {
            continue;
        };
        if !mentions_reserved(&field.data_type) {
            continue;
        }
        let typed = crate::executor::scan::try_json_to_typed_value(value, &field.data_type)?;
        validate_nested(&typed).map_err(|error| match error {
            QueryError::Cassie(error) => error,
            other => CassieError::Execution(other.to_string()),
        })?;
    }
    Ok(())
}
