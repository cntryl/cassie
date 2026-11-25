//! Admit complete native function signatures before allocating their diagnostic.
use super::admission::{check_controls, memory_error};
use super::{
    occurrences::{Occurrences, Summary},
    ParameterAdmissionError,
};
use crate::runtime::QueryExecutionControls;
use std::fmt::Write;

pub(super) fn admit(
    capacity: usize,
    sqlstate: &'static str,
    controls: &QueryExecutionControls,
    write: impl FnOnce(&mut String),
) -> Result<ParameterAdmissionError, ParameterAdmissionError> {
    check_controls(controls)?;
    let mut reservation = controls
        .reserve_query_memory(capacity)
        .map_err(memory_error)?;
    let mut message = String::new();
    message
        .try_reserve_exact(capacity)
        .map_err(|error| ParameterAdmissionError::Memory(error.to_string()))?;
    if message.capacity() > capacity {
        reservation
            .try_grow(message.capacity() - capacity)
            .map_err(memory_error)?;
    }
    check_controls(controls)?;
    write(&mut message);
    check_controls(controls)?;
    Ok(ParameterAdmissionError::FunctionDiagnostic {
        sqlstate,
        message,
        _reservation: reservation,
    })
}
impl Occurrences<'_> {
    pub(super) fn function_signature_error(
        &self,
        name: &str,
        arguments: &[Summary],
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        self.resolution_error(name, arguments, "42883", ") does not exist", controls)
    }
    pub(super) fn resolution_error(
        &self,
        name: &str,
        arguments: &[Summary],
        sqlstate: &'static str,
        suffix: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        if !self.stops_on_error() {
            return Ok(());
        }
        let mut capacity = name
            .len()
            .checked_add("function (".len() + suffix.len())
            .ok_or_else(overflow)?;
        for argument in arguments {
            check_controls(controls)?;
            capacity = capacity
                .checked_add(super::diagnostics::type_name(argument.oid).len() + 2)
                .ok_or_else(overflow)?;
        }
        Err(admit(capacity, sqlstate, controls, |message| {
            message.push_str("function ");
            let start = message.len();
            message.push_str(name);
            message[start..].make_ascii_lowercase();
            message.push('(');
            for (index, argument) in arguments.iter().enumerate() {
                if index != 0 {
                    message.push_str(", ");
                }
                write!(message, "{}", super::diagnostics::type_name(argument.oid))
                    .expect("String write");
            }
            message.push_str(suffix);
        })?)
    }
    pub(super) fn bytea_literal(
        &self,
        input: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        check_controls(controls)?;
        if self.stops_on_error() {
            if let Err(error) = crate::types::bytea_input::validate(input) {
                return Err(ParameterAdmissionError::ByteaInput(error));
            }
        }
        Ok(())
    }
}
fn overflow() -> ParameterAdmissionError {
    ParameterAdmissionError::Memory("function diagnostic capacity overflow".into())
}
