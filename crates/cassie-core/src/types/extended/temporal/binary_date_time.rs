//! Checked DATE and TIME binary input.
use super::calendar::{is_valid_date, DATE_NOBEGIN, DATE_NOEND, USECS_PER_DAY};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[doc(hidden)]
pub enum DateTimeBinaryError {
    InvalidFrame,
    DateOutOfRange,
    TimeOutOfRange,
}

impl DateTimeBinaryError {
    #[doc(hidden)]
    pub const fn sqlstate(self) -> &'static str {
        match self {
            Self::InvalidFrame => "08P01",
            Self::DateOutOfRange | Self::TimeOutOfRange => "22008",
        }
    }

    #[doc(hidden)]
    pub const fn wire_message(self) -> &'static str {
        match self {
            Self::InvalidFrame => "insufficient data left in message",
            Self::DateOutOfRange => "date out of range",
            Self::TimeOutOfRange => "time out of range",
        }
    }

    #[doc(hidden)]
    pub fn public_message(self, name: &str) -> String {
        match self {
            Self::InvalidFrame => format!("invalid binary {name} frame"),
            Self::DateOutOfRange | Self::TimeOutOfRange => self.wire_message().to_owned(),
        }
    }
}

impl std::fmt::Display for DateTimeBinaryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.wire_message())
    }
}
impl std::error::Error for DateTimeBinaryError {}

#[doc(hidden)]
pub fn date(bytes: &[u8]) -> Result<i32, DateTimeBinaryError> {
    let frame: [u8; 4] = bytes
        .try_into()
        .map_err(|_| DateTimeBinaryError::InvalidFrame)?;
    let value = i32::from_be_bytes(frame);
    if matches!(value, DATE_NOBEGIN | DATE_NOEND) || is_valid_date(i64::from(value)) {
        Ok(value)
    } else {
        Err(DateTimeBinaryError::DateOutOfRange)
    }
}

#[doc(hidden)]
pub fn time(bytes: &[u8]) -> Result<i64, DateTimeBinaryError> {
    let frame: [u8; 8] = bytes
        .try_into()
        .map_err(|_| DateTimeBinaryError::InvalidFrame)?;
    let value = i64::from_be_bytes(frame);
    if (0..=USECS_PER_DAY).contains(&value) {
        Ok(value)
    } else {
        Err(DateTimeBinaryError::TimeOutOfRange)
    }
}
