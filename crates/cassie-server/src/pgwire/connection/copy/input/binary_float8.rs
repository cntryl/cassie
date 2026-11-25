//! Fixed-stack FLOAT8 output for admitted binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if oid != 701 {
        return Ok(None);
    }
    controls.check_interruption()?;
    let value = super::super::super::codecs::binary_to_value(bytes, oid).map_err(|error| {
        CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column + 1
        ))
    })?;
    controls.check_interruption()?;
    let Value::Float64(value) = value else {
        return Err(CassieError::Execution(
            "unexpected binary COPY float8 value".into(),
        ));
    };
    let mut output = crate::types::float_text::StackText::new();
    crate::types::float_text::format_to(value, &mut output)
        .map_err(|_| CassieError::Execution("binary COPY float8 output exceeded stack".into()))?;
    let formatted = output
        .as_str()
        .map_err(|_| CassieError::Execution("invalid binary COPY float8 output".into()))?;
    let memory = controls.reserve_query_memory(formatted.len())?;
    let mut text = String::new();
    text.try_reserve_exact(formatted.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > formatted.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY float8 capacity".into(),
        ));
    }
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
