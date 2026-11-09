//! Preserve TIMESTAMP and TIMESTAMPTZ binary errors through Bind and COPY.
use std::io;

use crate::types::extended::{temporal::binary_timestamp::TimestampBinaryError, TypeInputError};
use crate::types::typed::TypedValue;
use crate::types::Value;

#[derive(Debug, thiserror::Error)]
#[error("improper binary format in array element {0}")]
struct TimestampArrayExcess(usize);

pub(super) fn decode(bytes: &[u8], zoned: bool) -> io::Result<Value> {
    let value = crate::types::extended::temporal::binary_timestamp::receive(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Value::Typed(if zoned {
        TypedValue::TimestampTz(value)
    } else {
        TypedValue::Timestamp(value)
    }))
}

fn scalar_excess(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error
        .downcast_ref::<super::ExcessBinaryData>()
        .is_some_and(|error| matches!(error.0, "timestamp" | "timestamptz"))
}

pub(super) fn array_error(error: io::Error, index: usize) -> io::Error {
    if error.get_ref().is_some_and(scalar_excess) {
        io::Error::new(io::ErrorKind::InvalidData, TimestampArrayExcess(index + 1))
    } else {
        error
    }
}

pub(super) fn sqlstate(
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> Option<&'static str> {
    if let Some(error) = error.downcast_ref::<TimestampBinaryError>() {
        Some(error.sqlstate())
    } else if error.is::<TimestampArrayExcess>() {
        Some("22P03")
    } else {
        None
    }
}

pub(super) fn copy_error(error: &io::Error) -> Option<TypeInputError> {
    let inner = error.get_ref()?;
    if let Some(error) = inner.downcast_ref::<TimestampBinaryError>() {
        Some(TypeInputError::new(
            error.sqlstate(),
            error.wire_message().to_owned(),
        ))
    } else if let Some(error) = inner.downcast_ref::<TimestampArrayExcess>() {
        Some(TypeInputError::new("22P03", error.to_string()))
    } else if scalar_excess(inner) {
        Some(TypeInputError::new(
            "22P03",
            "incorrect binary data format".to_owned(),
        ))
    } else {
        None
    }
}
