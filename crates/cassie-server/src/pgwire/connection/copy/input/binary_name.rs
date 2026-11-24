//! Direct admitted NAME COPY output preserving its native binary receive rules.

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if oid != 19 {
        return Ok(None);
    }
    controls.check_interruption()?;
    let text = if bytes.len() <= crate::types::name::MAX_NAME_BYTES {
        std::str::from_utf8(bytes).ok()
    } else {
        None
    };
    let Some(text) = text else {
        let checked = &bytes[..bytes.len().min(64)];
        let error = super::super::super::codecs::binary_to_value(checked, oid)
            .expect_err("invalid NAME input must be rejected");
        return Err(CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column + 1
        )));
    };
    let memory = controls.reserve_query_memory(text.len())?;
    let mut output = String::new();
    output
        .try_reserve_exact(text.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > text.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY NAME capacity".into(),
        ));
    }
    output.push_str(text);
    controls.check_interruption()?;
    Ok(Some((output, memory)))
}
