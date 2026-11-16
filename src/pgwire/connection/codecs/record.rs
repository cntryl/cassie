//! Wire text and binary output for record values (`record_out` / `record_send`).
use std::io;

use crate::types::typed::TypedValue;
use crate::types::{DataType, RecordValue, Value};

/// The record carried by `value`, if it is one.
pub(super) const fn as_record(value: &Value) -> Option<&RecordValue> {
    match value {
        Value::Typed(TypedValue::Record(record)) => Some(record),
        _ => None,
    }
}

/// The record a rendered record cell stands for. Results carry records as
/// their text; a column with a named record type parses it back for binary
/// output. `None` for every other cell.
pub(in crate::pgwire::connection) fn from_rendered(
    value: &Value,
    column: &crate::executor::ColumnMeta,
) -> io::Result<Option<Value>> {
    let (Value::String(text), Some(descriptor)) = (value, column.record_type.as_ref()) else {
        return Ok(None);
    };
    crate::executor::filter::record_from_text(text, descriptor)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

/// PostgreSQL record text output with each field rendered by the wire codec
/// of the field's own type.
pub(super) fn text(record: &RecordValue) -> String {
    crate::types::composite::format_record_with(record, &mut |value, data_type| {
        let field_oid = data_type.type_oid();
        super::value_to_text(value, field_oid)
    })
}

/// PostgreSQL binary record output (`record_send`).
pub(super) fn binary(record: &RecordValue) -> io::Result<Vec<u8>> {
    crate::types::composite::send_record_binary(record, |value, data_type| {
        // A field that is an array of a named composite type (`graph_path.vertices`) nests
        // its own `array_send` of `record_send` elements.
        if let DataType::Array(element) = data_type.flattened() {
            if let DataType::Composite(descriptor) = element.flattened() {
                if !value.is_null() {
                    return super::record_array::encode_nested(value, descriptor)
                        .map_err(|error| RecordSendError(error.to_string()));
                }
            }
        }
        let field_oid = data_type.type_oid();
        super::value_to_binary(value, field_oid).map_err(|error| RecordSendError(error.to_string()))
    })
    .map_err(|error: RecordSendError| io::Error::new(io::ErrorKind::InvalidData, error.0))
}

struct RecordSendError(String);

impl From<String> for RecordSendError {
    fn from(message: String) -> Self {
        Self(message)
    }
}
