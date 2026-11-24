//! PostgreSQL's typed error for an overlength BYTEA-to-integer conversion.
use super::IntegerWidth;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntegerByteaError {
    /// Target signed integer domain.
    pub width: IntegerWidth,
    /// Number of input bytes rejected before conversion.
    pub byte_count: usize,
}

impl IntegerByteaError {
    pub(crate) const fn new(width: IntegerWidth, byte_count: usize) -> Self {
        Self { width, byte_count }
    }
}

impl std::fmt::Display for IntegerByteaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.width.range_message())
    }
}

impl std::error::Error for IntegerByteaError {}
