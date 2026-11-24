//! Own copied scalar diagnostics only after admitting their input buffer.
use super::admission::{check_controls, memory_error};
use super::diagnostics::ScalarLiteralDiagnostic;
use super::ParameterAdmissionError;
use crate::runtime::QueryExecutionControls;

pub(super) fn admit(
    input: &str,
    diagnostic: ScalarLiteralDiagnostic,
    controls: &QueryExecutionControls,
) -> Result<ParameterAdmissionError, ParameterAdmissionError> {
    check_controls(controls)?;
    let requested = input.len();
    let mut reservation = controls
        .reserve_query_memory(requested)
        .map_err(memory_error)?;
    let mut admitted = String::new();
    admitted.try_reserve_exact(requested).map_err(|error| {
        ParameterAdmissionError::Memory(format!(
            "unable to allocate scalar input diagnostic: {error}"
        ))
    })?;
    if admitted.capacity() > requested {
        reservation
            .try_grow(admitted.capacity() - requested)
            .map_err(memory_error)?;
    }
    check_controls(controls)?;
    admitted.push_str(input);
    Ok(ParameterAdmissionError::InvalidScalarLiteral {
        input: admitted,
        diagnostic,
        _reservation: reservation,
    })
}
