//! Bounded COPY binary formatting for fixed-width boolean and integer families.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if !matches!(oid, 16 | 20 | 21 | 23 | 26) {
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
    let mut storage = [0_u8; 21];
    let start = match value {
        Value::Bool(value) => {
            storage[20] = if value { b't' } else { b'f' };
            20
        }
        Value::Int64(value) => {
            let negative = value < 0;
            let mut magnitude = value.unsigned_abs();
            let mut start = storage.len();
            loop {
                start -= 1;
                storage[start] = b'0' + u8::try_from(magnitude % 10).expect("decimal digit");
                magnitude /= 10;
                if magnitude == 0 {
                    break;
                }
            }
            if negative {
                start -= 1;
                storage[start] = b'-';
            }
            start
        }
        _ => {
            return Err(CassieError::Execution(
                "unexpected fixed binary COPY value".into(),
            ))
        }
    };
    let raw = &storage[start..];
    let memory = controls.reserve_query_memory(raw.len())?;
    let mut output = String::new();
    output
        .try_reserve_exact(raw.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > raw.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY binary fixed capacity".into(),
        ));
    }
    let text = std::str::from_utf8(raw)
        .map_err(|_| CassieError::Execution("invalid binary COPY numeric output".into()))?;
    output.push_str(text);
    controls.check_interruption()?;
    Ok(Some((output, memory)))
}
