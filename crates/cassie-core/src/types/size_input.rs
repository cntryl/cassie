//! Typed failures for PostgreSQL human-readable byte-size input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SizeInputError {
    #[error("invalid size: \"{0}\"")]
    InvalidSize(String),
    #[error("invalid input syntax for type numeric: \"{0}\"")]
    InvalidNumeric(String),
    #[error("value overflows numeric format")]
    NumericOverflow,
    #[error("bigint out of range")]
    BigIntOverflow,
}

impl SizeInputError {
    #[must_use]
    pub const fn sql_state(&self) -> &'static str {
        match self {
            Self::InvalidSize(_) => "22023",
            Self::InvalidNumeric(_) => "22P02",
            Self::NumericOverflow | Self::BigIntOverflow => "22003",
        }
    }
}
