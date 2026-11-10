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

pub(crate) fn copy_text(
    value: &str,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(String, crate::runtime::QueryMemoryReservation), CassieError> {
    controls.check_interruption()?;
    let memory = controls.reserve_query_memory(value.len())?;
    let mut copy = String::new();
    copy.try_reserve_exact(value.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if copy.capacity() > value.len() {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY text capacity".into(),
        ));
    }
    let mut start = 0;
    while start < value.len() {
        controls.check_interruption()?;
        let mut end = (start + 4096).min(value.len());
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        copy.push_str(&value[start..end]);
        start = end;
    }
    Ok((copy, memory))
}
