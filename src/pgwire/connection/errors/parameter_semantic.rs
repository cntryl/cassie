//! Admit the selected copied diagnostic and its complete network frame before allocation.

use std::fmt::{self, Write};
use std::mem::size_of;

use super::{PgWireError, PgWireSeverity};
use crate::app::CassieError;
use crate::executor::retained_memory::add;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ParameterSemanticDiagnostic;

#[derive(Debug)]
pub(super) struct AdmittedErrorFrame {
    bytes: Vec<u8>,
    // The frame, enclosing Strings and enclosing error box retire before this lease.
    _memory: QueryMemoryReservation,
}

impl PgWireError {
    pub(in crate::pgwire::connection) fn parameter_semantic(
        diagnostic: ParameterSemanticDiagnostic,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let mut memory = controls.reserve_query_memory(size_of::<Self>())?;
        let mut message = admitted_string(diagnostic.message_capacity(), controls, &mut memory)?;
        diagnostic
            .write_message(&mut BoundedOutput(&mut message))
            .map_err(|_| formatting_error())?;
        let detail = match diagnostic.detail_capacity() {
            Some(capacity) => {
                let mut detail = admitted_string(capacity, controls, &mut memory)?;
                diagnostic
                    .write_detail(&mut BoundedOutput(&mut detail))
                    .map_err(|_| formatting_error())?;
                Some(detail)
            }
            None => None,
        };
        let mut error = Self::new(PgWireSeverity::Error, diagnostic.code(), message);
        error.detail = detail;
        admitted_error_frame(error, controls, memory)
    }

    pub(in crate::pgwire::connection) fn invalid_boolean_input(
        input: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let mut memory = controls.reserve_query_memory(size_of::<Self>())?;
        let prefix = "invalid input syntax for type boolean: ";
        let capacity = prefix
            .len()
            .checked_add(input.len())
            .and_then(|length| length.checked_add(2))
            .ok_or_else(capacity_error)?;
        let mut message = admitted_string(capacity, controls, &mut memory)?;
        write!(&mut BoundedOutput(&mut message), "{prefix}\"{input}\"")
            .map_err(|_| formatting_error())?;
        let error = Self::new(PgWireSeverity::Error, "22P02", message);
        admitted_error_frame(error, controls, memory)
    }

    pub(in crate::pgwire::connection) fn nonaggregate_modifier(
        message: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let mut memory = controls.reserve_query_memory(size_of::<Self>())?;
        let mut admitted = admitted_string(message.len(), controls, &mut memory)?;
        admitted.push_str(message);
        let error = Self::new(PgWireSeverity::Error, "42809", admitted);
        admitted_error_frame(error, controls, memory)
    }

    pub(in crate::pgwire::connection) fn encoded_semantic_frame(&self) -> Option<&[u8]> {
        self.semantic_frame
            .as_ref()
            .map(|frame| frame.bytes.as_slice())
    }
}

fn admitted_error_frame(
    mut error: PgWireError,
    controls: &QueryExecutionControls,
    mut memory: QueryMemoryReservation,
) -> Result<PgWireError, CassieError> {
    // Each tagged C string costs its tag and terminating NUL; the payload has a final NUL.
    let mut payload_size = 1;
    for value in [error.severity.as_str(), error.code, error.message.as_str()] {
        payload_size = add(payload_size, add(value.len(), 2)?)?;
    }
    if let Some(detail) = &error.detail {
        payload_size = add(payload_size, add(detail.len(), 2)?)?;
    }
    let mut payload = admitted_bytes(payload_size, controls, &mut memory)?;
    for (tag, value) in [
        (b'S', Some(error.severity.as_str())),
        (b'C', Some(error.code)),
        (b'M', Some(error.message.as_str())),
        (b'D', error.detail.as_deref()),
    ] {
        if let Some(value) = value {
            payload.push(tag);
            payload.extend_from_slice(value.as_bytes());
            payload.push(0);
        }
    }
    payload.push(0);
    let mut bytes = admitted_bytes(add(payload_size, 5)?, controls, &mut memory)?;
    super::super::writers::append_backend_frame(&mut bytes, b'E', &payload)
        .map_err(|error| CassieError::ResourceLimit(error.to_string()))?;
    let payload_capacity = payload.capacity();
    drop(payload);
    memory.shrink_to(memory.bytes() - payload_capacity);
    controls.check_interruption()?;
    error.semantic_frame = Some(AdmittedErrorFrame {
        bytes,
        _memory: memory,
    });
    Ok(error)
}

fn admitted_string(
    capacity: usize,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<String, CassieError> {
    controls.check_interruption()?;
    memory.try_grow(capacity)?;
    controls.check_interruption()?;
    let mut value = String::new();
    value.try_reserve_exact(capacity).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to allocate parameter diagnostic: {error}"))
    })?;
    reconcile_capacity(value.capacity(), capacity, controls, memory)?;
    Ok(value)
}

fn admitted_bytes(
    capacity: usize,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<Vec<u8>, CassieError> {
    controls.check_interruption()?;
    memory.try_grow(capacity)?;
    controls.check_interruption()?;
    let mut value = Vec::new();
    value.try_reserve_exact(capacity).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to allocate parameter error frame: {error}"))
    })?;
    reconcile_capacity(value.capacity(), capacity, controls, memory)?;
    Ok(value)
}

fn reconcile_capacity(
    actual: usize,
    requested: usize,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    if actual > requested {
        memory.try_grow(actual - requested)?;
    }
    Ok(())
}

fn formatting_error() -> CassieError {
    CassieError::Planner("unable to format bounded parameter diagnostic".to_owned())
}

fn capacity_error() -> CassieError {
    CassieError::ResourceLimit("parameter diagnostic capacity overflow".to_owned())
}

struct BoundedOutput<'a>(&'a mut String);

impl Write for BoundedOutput<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if value.len() > self.0.capacity() - self.0.len() {
            return Err(fmt::Error);
        }
        self.0.push_str(value);
        Ok(())
    }
}
