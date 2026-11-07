//! Shared capacity-coupled COPY decoder helpers.
use super::CassieError;

pub(crate) fn validate_payload_utf8(
    payload: &[u8],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), CassieError> {
    let mut offset = 0;
    while offset < payload.len() {
        controls.check_interruption()?;
        let mut end = (offset + 4096).min(payload.len());
        while end > offset && end < payload.len() && payload[end] & 0xc0 == 0x80 {
            end -= 1;
        }
        if end == offset || std::str::from_utf8(&payload[offset..end]).is_err() {
            return Err(CassieError::Parse("COPY payload must be UTF-8".into()));
        }
        offset = end;
    }
    Ok(())
}

pub(crate) fn grow_text_vec<T>(
    values: &mut Vec<T>,
    memory: &mut crate::runtime::QueryMemoryReservation,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    if values.len() < values.capacity() {
        return Ok(());
    }
    let capacity = values
        .len()
        .checked_add(1)
        .and_then(|n| n.max(4).checked_next_power_of_two())
        .ok_or_else(|| CassieError::ResourceLimit("COPY row capacity overflow".into()))?;
    let old_bytes = values
        .capacity()
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| CassieError::ResourceLimit("COPY row capacity overflow".into()))?;
    let replacement_memory;
    let mut replacement;
    (replacement, replacement_memory) =
        crate::runtime::owned_memory::try_reserve_vec(capacity, memory)?;
    for (index, value) in values.drain(..).enumerate() {
        if index % 128 == 0 {
            controls.check_interruption()?;
        }
        replacement.push(value);
    }
    let old = std::mem::replace(values, replacement);
    drop(old);
    memory.shrink_to(memory.bytes() - old_bytes);
    memory.absorb(replacement_memory);
    Ok(())
}
