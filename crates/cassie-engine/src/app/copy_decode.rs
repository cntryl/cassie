//! Shared capacity-coupled COPY decoder helpers.
use super::CassieError;

#[doc(hidden)]
pub fn validate_payload_utf8(
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

#[doc(hidden)]
pub fn grow_text_vec<T>(
    values: &mut Vec<T>,
    memory: &mut crate::runtime::QueryMemoryReservation,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), CassieError> {
    let request = cassie_core::request::text::TextRequest::new(
        controls.interruption(),
        controls.request_memory(),
    );
    cassie_core::request::text::grow_text_vec(values, memory.as_core_mut(), &request)
        .map_err(CassieError::from)
}

#[doc(hidden)]
pub fn copy_text(
    value: &str,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(String, crate::runtime::QueryMemoryReservation), CassieError> {
    let request = cassie_core::request::text::TextRequest::new(
        controls.interruption(),
        controls.request_memory(),
    );
    let (text, memory) = cassie_core::request::text::copy_text(value, &request)?;
    Ok((text, crate::runtime::QueryMemoryReservation::from_core(memory)))
}

impl From<cassie_core::request::text::TextError> for CassieError {
    fn from(error: cassie_core::request::text::TextError) -> Self {
        use cassie_core::request::{text::TextError, InterruptionError};
        match error {
            TextError::Interrupted(InterruptionError::Canceled) => Self::QueryCanceled,
            TextError::Interrupted(InterruptionError::DeadlineExceeded) => Self::DeadlineExceeded,
            TextError::ResourceLimit(message) => Self::ResourceLimit(message),
        }
    }
}
