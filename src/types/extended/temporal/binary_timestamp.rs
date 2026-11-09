//! Checked TIMESTAMP and TIMESTAMPTZ binary input.
use super::calendar::{is_valid_timestamp, TIMESTAMP_NOBEGIN, TIMESTAMP_NOEND};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimestampBinaryError {
    InvalidFrame,
    OutOfRange,
}

impl TimestampBinaryError {
    pub(crate) const fn sqlstate(self) -> &'static str {
        match self {
            Self::InvalidFrame => "08P01",
            Self::OutOfRange => "22008",
        }
    }

    pub(crate) const fn wire_message(self) -> &'static str {
        match self {
            Self::InvalidFrame => "insufficient data left in message",
            Self::OutOfRange => "timestamp out of range",
        }
    }

    pub(crate) fn public_message(self, name: &str) -> String {
        match self {
            Self::InvalidFrame => format!("invalid binary {name} frame"),
            Self::OutOfRange => self.wire_message().to_owned(),
        }
    }
}

impl std::fmt::Display for TimestampBinaryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.wire_message())
    }
}

impl std::error::Error for TimestampBinaryError {}

pub(crate) fn receive(bytes: &[u8]) -> Result<i64, TimestampBinaryError> {
    let frame: [u8; 8] = bytes
        .try_into()
        .map_err(|_| TimestampBinaryError::InvalidFrame)?;
    let value = i64::from_be_bytes(frame);
    if matches!(value, TIMESTAMP_NOBEGIN | TIMESTAMP_NOEND) || is_valid_timestamp(value) {
        Ok(value)
    } else {
        Err(TimestampBinaryError::OutOfRange)
    }
}
