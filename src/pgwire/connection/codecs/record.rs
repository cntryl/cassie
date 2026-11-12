//! Wire text and binary output for record values (`record_out` / `record_send`).
use std::io;

use crate::types::typed::TypedValue;
use crate::types::{RecordValue, Value};

/// The record carried by `value`, if it is one.
pub(super) const fn as_record(value: &Value) -> Option<&RecordValue> {
    match value {
        Value::Typed(TypedValue::Record(record)) => Some(record),
        _ => None,
    }
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
