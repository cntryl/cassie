//! Preserve TIMETZ binary receive errors through scalar and array COPY.
use std::io;

use crate::types::extended::{temporal::TimeTzBinaryError, TypeInputError};

#[derive(Debug, thiserror::Error)]
#[error("improper binary format in array element {0}")]
struct TimetzArrayExcess(usize);

pub(super) fn scalar_excess(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error
        .downcast_ref::<super::ExcessBinaryData>()
        .is_some_and(|error| error.0 == "timetz")
}

pub(super) fn array_error(error: io::Error, index: usize) -> io::Error {
    if error.get_ref().is_some_and(scalar_excess) {
        io::Error::new(io::ErrorKind::InvalidData, TimetzArrayExcess(index + 1))
    } else {
        error
    }
}

pub(super) fn sqlstate(
    error: &(dyn std::error::Error + Send + Sync + 'static),
) -> Option<&'static str> {
    if let Some(error) = error.downcast_ref::<TimeTzBinaryError>() {
        Some(error.sqlstate())
    } else if error.is::<TimetzArrayExcess>() {
        Some("22P03")
    } else {
        None
    }
}

pub(super) fn copy_error(error: &io::Error) -> Option<TypeInputError> {
    let inner = error.get_ref()?;
    if let Some(error) = inner.downcast_ref::<TimeTzBinaryError>() {
        Some(TypeInputError::new(
            error.sqlstate(),
            error.wire_message().to_owned(),
        ))
    } else if let Some(error) = inner.downcast_ref::<TimetzArrayExcess>() {
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
