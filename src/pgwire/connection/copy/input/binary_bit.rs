//! Admitted BIT and VARBIT output for binary COPY fields.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "binary_bit_tests.rs"]
mod tests;

fn valid_length(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let (length, payload) = bytes.split_first_chunk::<4>()?;
    let length = usize::try_from(i32::from_be_bytes(*length)).ok()?;
    (payload.len() == length.div_ceil(8)).then_some((length, payload))
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    decode_with_observer(oid, bytes, column, controls, || {})
}

#[cfg(test)]
fn decode_observed(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    observer: impl FnMut(),
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    decode_with_observer(oid, bytes, column, controls, observer)
}

fn decode_with_observer(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
    mut observer: impl FnMut(),
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if !matches!(oid, 1560 | 1562) {
        return Ok(None);
    }
    controls.check_interruption()?;
    let Some((length, payload)) = valid_length(bytes) else {
        // Each failed precheck reaches an error in receive_binary before its
        // payload Vec allocation; retain the original error and priority.
        let error = match super::super::super::codecs::binary_to_value(bytes, oid) {
            Err(error) => error,
            Ok(_) => {
                return Err(CassieError::Execution(
                    "binary COPY bit validation disagrees with codec".into(),
                ));
            }
        };
        return Err(CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column + 1
        )));
    };
    let memory = controls.reserve_query_memory(length)?;
    let mut text = String::new();
    text.try_reserve_exact(length)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY bit capacity".into(),
        ));
    }
    for index in 0..length {
        if index.is_multiple_of(4096) {
            if index != 0 {
                observer();
            }
            controls.check_interruption()?;
        }
        text.push(if payload[index / 8] & (0x80 >> (index % 8)) == 0 {
            '0'
        } else {
            '1'
        });
    }
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
