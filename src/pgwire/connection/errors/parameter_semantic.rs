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

const COPY_UNDEFINED_PREFIX: &str = "unresolvable column reference '";
const COPY_UNDEFINED_SUFFIX: &str = "'; known fields or aliases required";

pub(super) fn copy_unknown_column_frame_sizes(
    name_len: usize,
) -> Result<(usize, usize, usize), CassieError> {
    let message_size = add(
        add(COPY_UNDEFINED_PREFIX.len(), name_len)?,
        COPY_UNDEFINED_SUFFIX.len(),
    )?;
    let payload_size = add(
        add(
            add(1, add(2, PgWireSeverity::Error.as_str().len())?)?,
            add(2, "42703".len())?,
        )?,
        add(2, message_size)?,
    )?;
    let frame_length = add(payload_size, 4)?;
    if frame_length > super::super::writers::MAX_BACKEND_FRAME_BYTES {
        return Err(CassieError::ResourceLimit(
            "pgwire backend frame exceeds 16777216 bytes".into(),
        ));
    }
    Ok((message_size, frame_length, add(frame_length, 1)?))
}

impl PgWireError {
    pub(in crate::pgwire::connection) fn copy_undefined_column_fits(name_len: usize) -> bool {
        copy_unknown_column_frame_sizes(name_len).is_ok()
    }

    pub(in crate::pgwire::connection) fn copy_undefined_column(
        name: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let (message_size, frame_length, frame_size) = copy_unknown_column_frame_sizes(name.len())?;
        let mut memory = controls.reserve_query_memory(size_of::<Self>())?;
        memory.try_grow(message_size)?;
        let mut message = String::new();
        message
            .try_reserve_exact(message_size)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if message.capacity() > message_size {
            return Err(capacity_error());
        }
        message.push_str(COPY_UNDEFINED_PREFIX);
        let mut start = 0;
        while start < name.len() {
            controls.check_interruption()?;
            let mut end = (start + 4096).min(name.len());
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            message.push_str(&name[start..end]);
            start = end;
        }
        message.push_str(COPY_UNDEFINED_SUFFIX);
        let mut error = Self::new(PgWireSeverity::Error, "42703", message);
        let encoded_length = i32::try_from(frame_length).map_err(|_| capacity_error())?;
        controls.check_interruption()?;
        memory.try_grow(frame_size)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(frame_size)
            .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
        if bytes.capacity() > frame_size {
            return Err(capacity_error());
        }
        bytes.push(b'E');
        bytes.extend_from_slice(&encoded_length.to_be_bytes());
        for (tag, value) in [
            (b'S', error.severity.as_str()),
            (b'C', error.code),
            (b'M', error.message.as_str()),
        ] {
            bytes.push(tag);
            for chunk in value.as_bytes().chunks(4096) {
                controls.check_interruption()?;
                bytes.extend_from_slice(chunk);
            }
            bytes.push(0);
        }
        bytes.push(0);
        controls.check_interruption()?;
        error.semantic_frame = Some(AdmittedErrorFrame {
            bytes,
            _memory: memory,
        });
        Ok(error)
    }

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

    pub(in crate::pgwire::connection) fn invalid_scalar_input(
        input: &str,
        diagnostic: crate::sql::ScalarLiteralDiagnostic,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let mut memory = controls.reserve_query_memory(size_of::<Self>())?;
        let capacity = diagnostic.capacity(input).ok_or_else(capacity_error)?;
        let mut message = admitted_string(capacity, controls, &mut memory)?;
        diagnostic
            .write(input, &mut BoundedOutput(&mut message))
            .map_err(|_| formatting_error())?;
        let error = Self::new(PgWireSeverity::Error, diagnostic.code(), message);
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

    pub(in crate::pgwire::connection) fn function_diagnostic(
        sqlstate: &'static str,
        message: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        Self::controlled_diagnostic(PgWireSeverity::Error, sqlstate, message, controls)
    }

    pub(super) fn controlled_diagnostic(
        severity: PgWireSeverity,
        sqlstate: &'static str,
        message: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        let mut memory = controls.reserve_query_memory(size_of::<Self>())?;
        let mut admitted = admitted_string(message.len(), controls, &mut memory)?;
        append_text_checked(&mut admitted, message, controls)?;
        let error = Self::new(severity, sqlstate, admitted);
        admitted_error_frame(error, controls, memory)
    }

    pub(in crate::pgwire::connection) fn encoded_semantic_frame(&self) -> Option<&[u8]> {
        self.semantic_frame
            .as_ref()
            .map(|frame| frame.bytes.as_slice())
    }
}

struct ErrorWithMemory {
    error: PgWireError,
    memory: QueryMemoryReservation,
}

fn append_text_checked(
    output: &mut String,
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    let mut start = 0;
    while start < input.len() {
        controls.check_interruption()?;
        let mut end = (start + 1024).min(input.len());
        while !input.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&input[start..end]);
        start = end;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameCopyPhase {
    Message,
    Frame,
}

fn append_bytes_checked(
    output: &mut Vec<u8>,
    input: &[u8],
    controls: &QueryExecutionControls,
    mut progress: impl FnMut(),
) -> Result<(), CassieError> {
    for chunk in input.chunks(1024) {
        controls.check_interruption()?;
        output.extend_from_slice(chunk);
        progress();
        controls.check_interruption()?;
    }
    Ok(())
}

fn admitted_error_frame(
    error: PgWireError,
    controls: &QueryExecutionControls,
    memory: QueryMemoryReservation,
) -> Result<PgWireError, CassieError> {
    admitted_error_frame_observing(error, controls, memory, |_| {})
}

pub(super) fn admitted_error_frame_observing(
    error: PgWireError,
    controls: &QueryExecutionControls,
    memory: QueryMemoryReservation,
    mut progress: impl FnMut(FrameCopyPhase),
) -> Result<PgWireError, CassieError> {
    // This owner keeps the message alive only while its reservation is alive,
    // including every allocation and interruption failure below.
    let mut owned = ErrorWithMemory { error, memory };
    let mut payload_size = 1;
    for value in [
        owned.error.severity.as_str(),
        owned.error.code,
        owned.error.message.as_str(),
    ] {
        payload_size = add(payload_size, add(value.len(), 2)?)?;
    }
    if let Some(detail) = &owned.error.detail {
        payload_size = add(payload_size, add(detail.len(), 2)?)?;
    }
    let frame_length = add(payload_size, 4)?;
    if frame_length > super::super::writers::MAX_BACKEND_FRAME_BYTES {
        return Err(CassieError::ResourceLimit(
            "pgwire backend frame exceeds 16777216 bytes".into(),
        ));
    }
    let mut payload = admitted_bytes(payload_size, controls, &mut owned.memory)?;
    for (tag, value) in [
        (b'S', Some(owned.error.severity.as_str())),
        (b'C', Some(owned.error.code)),
        (b'M', Some(owned.error.message.as_str())),
        (b'D', owned.error.detail.as_deref()),
    ] {
        if let Some(value) = value {
            payload.push(tag);
            append_bytes_checked(&mut payload, value.as_bytes(), controls, || {
                if tag == b'M' {
                    progress(FrameCopyPhase::Message);
                }
            })?;
            payload.push(0);
        }
    }
    payload.push(0);
    let mut bytes = admitted_bytes(add(payload_size, 5)?, controls, &mut owned.memory)?;
    let encoded_length = i32::try_from(frame_length)
        .map_err(|_| CassieError::ResourceLimit("pgwire backend frame length overflow".into()))?;
    bytes.push(b'E');
    bytes.extend_from_slice(&encoded_length.to_be_bytes());
    append_bytes_checked(&mut bytes, &payload, controls, || {
        progress(FrameCopyPhase::Frame)
    })?;
    let payload_capacity = payload.capacity();
    drop(payload);
    owned
        .memory
        .shrink_to(owned.memory.bytes() - payload_capacity);
    controls.check_interruption()?;
    let ErrorWithMemory { mut error, memory } = owned;
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
    reconcile_capacity(value.capacity(), capacity, controls)?;
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
    reconcile_capacity(value.capacity(), capacity, controls)?;
    Ok(value)
}

fn reconcile_capacity(
    actual: usize,
    requested: usize,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    if actual > requested {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted parameter diagnostic capacity".into(),
        ));
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
