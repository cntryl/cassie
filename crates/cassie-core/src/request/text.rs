//! Admission and interruption for retained request text.

use super::accounted::allocation;
use super::{Interruption, InterruptionError, RequestMemory, RequestMemoryReservation};

/// The existing resource and interruption authorities for a text operation.
pub struct TextRequest<'a> {
    interruption: Interruption<'a>,
    memory: &'a RequestMemory,
}

#[derive(Debug)]
pub enum TextError {
    Interrupted(InterruptionError),
    ResourceLimit(String),
}

impl<'a> TextRequest<'a> {
    #[must_use]
    pub const fn new(interruption: Interruption<'a>, memory: &'a RequestMemory) -> Self {
        Self { interruption, memory }
    }

    pub(crate) fn check_interruption(&self) -> Result<(), TextError> {
        self.interruption.check().map_err(TextError::Interrupted)
    }

    pub(crate) fn reserve(&self, bytes: usize) -> Result<RequestMemoryReservation, TextError> {
        self.memory.reserve(bytes).map_err(|error| {
            TextError::ResourceLimit(format!(
                "query memory budget exceeded: {} > {}",
                error.requested, error.budget
            ))
        })
    }
}

pub(crate) fn allocation_error(error: std::collections::TryReserveError) -> TextError {
    TextError::ResourceLimit(allocation::allocation_error(error).to_string())
}

/// Grows the directory while retaining the old allocation and polling for interruption.
///
/// # Errors
/// Returns an interruption or admission error.
pub fn grow_text_vec<T>(
    values: &mut Vec<T>,
    memory: &mut RequestMemoryReservation,
    controls: &TextRequest<'_>,
) -> Result<(), TextError> {
    controls.check_interruption()?;
    if values.len() < values.capacity() {
        return Ok(());
    }
    let capacity = values
        .len()
        .checked_add(1)
        .and_then(|n| n.max(4).checked_next_power_of_two())
        .ok_or_else(|| TextError::ResourceLimit("COPY row capacity overflow".into()))?;
    let old_bytes = values
        .capacity()
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(|| TextError::ResourceLimit("COPY row capacity overflow".into()))?;
    let (mut replacement, replacement_memory) = allocation::try_reserve_vec(capacity, memory)
        .map_err(|error| TextError::ResourceLimit(error.to_string()))?;
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

/// Copies UTF-8 text under its original request reservation.
///
/// # Errors
/// Returns an interruption or admission error.
pub fn copy_text(
    value: &str,
    controls: &TextRequest<'_>,
) -> Result<(String, RequestMemoryReservation), TextError> {
    controls.check_interruption()?;
    let memory = controls.reserve(value.len())?;
    let mut copy = String::new();
    copy.try_reserve_exact(value.len())
        .map_err(allocation_error)?;
    if copy.capacity() > value.len() {
        return Err(TextError::ResourceLimit(
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
