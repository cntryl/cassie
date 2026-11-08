//! Direct admitted UUID COPY output from its fixed-width binary representation.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if oid != 2950 {
        return Ok(None);
    }
    controls.check_interruption()?;
    if bytes.len() != 16 {
        let checked = &bytes[..bytes.len().min(17)];
        let error = super::super::super::codecs::binary_to_value(checked, oid)
            .expect_err("invalid UUID binary width must be rejected");
        return Err(CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column + 1
        )));
    }
    let raw: [u8; 16] = bytes.try_into().expect("UUID length checked above");
    let uuid = uuid::Uuid::from_bytes(raw);
    let mut storage = uuid::Uuid::encode_buffer();
    let text = uuid.hyphenated().encode_lower(&mut storage);
    let memory = controls.reserve_query_memory(text.len())?;
    let mut output = String::new();
    output
        .try_reserve_exact(text.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > text.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY UUID capacity".into(),
        ));
    }
    output.push_str(text);
    controls.check_interruption()?;
    Ok(Some((output, memory)))
}
