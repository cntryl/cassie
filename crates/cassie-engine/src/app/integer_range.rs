//! Integer column range validation for document writes.
//!
//! An integer outside a column's range is `22003 numeric_value_out_of_range`
//! with PostgreSQL's message, not a vector-shape error.

use super::{CassieError, DiagnosticError};

fn out_of_range(type_name: &str) -> CassieError {
    DiagnosticError::new("22003", format!("{type_name} out of range")).into()
}

fn expects(_field: &str, type_name: &str) -> CassieError {
    DiagnosticError::new(
        "22P02",
        format!("invalid input syntax for type {type_name}"),
    )
    .into()
}

/// Whether `value` is a JSON number holding a whole value too large for `i64`.
fn is_wide_integer(value: &serde_json::Value) -> bool {
    value.is_u64() || value.as_f64().is_some_and(|number| number.fract() == 0.0)
}

/// Validates a `SMALLINT` or `INT` cell against `in_range`.
///
/// `type_name` is the name in the "expects" diagnostic and `range_name` the
/// one in PostgreSQL's "out of range" message.
pub(super) fn validate_in_range<F>(
    field: &str,
    value: &serde_json::Value,
    type_name: &str,
    range_name: &str,
    in_range: F,
) -> Result<(), CassieError>
where
    F: FnOnce(i64) -> bool,
{
    if let Some(number) = value.as_i64() {
        return if in_range(number) {
            Ok(())
        } else {
            Err(out_of_range(range_name))
        };
    }
    if is_wide_integer(value) {
        return Err(out_of_range(range_name));
    }
    Err(expects(field, type_name))
}

/// Validates a `BIGINT` cell.
pub(super) fn validate_bigint(field: &str, value: &serde_json::Value) -> Result<(), CassieError> {
    if value.is_i64() {
        Ok(())
    } else if is_wide_integer(value) {
        Err(out_of_range("bigint"))
    } else {
        Err(expects(field, "bigint"))
    }
}
