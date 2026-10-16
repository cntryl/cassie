//! Ordered semantic admission before prepared-statement publication.
use super::{unknown_high_bit_projection_oid, with_function_input_scope, ExtendedQueryError};
use crate::app::{Cassie, CassieSession};
use crate::runtime::QueryExecutionControls;
use crate::sql::{AdmittedParameterTypes, ParsedStatement};

pub(super) fn admit(
    cassie: &Cassie,
    session: &CassieSession,
    parsed: &mut ParsedStatement,
    parameter_type_oids: &[i32],
    controls: &QueryExecutionControls,
) -> Result<AdmittedParameterTypes, ExtendedQueryError> {
    let admitted = with_function_input_scope(cassie, session, |scope| {
        crate::sql::checked_wire_parse_parameter_types_in_scope(
            parsed,
            parameter_type_oids,
            &cassie.catalog,
            controls,
            Some(scope),
        )
    })
    .map_err(|error| ExtendedQueryError::parameter_admission(error, controls))?;
    // Bind source/name/input semantics without planning, before final slot
    // validation and before making the prepared statement visible.
    crate::sql::parse_analysis::validate(
        parsed,
        admitted.types(),
        &cassie.catalog,
        &cassie.binding_context_for_session(Some(session)),
        controls,
    )
    .map_err(|error| ExtendedQueryError::cassie(&error))?;
    if let Some(oid) = unknown_high_bit_projection_oid(parsed, admitted.types()) {
        return Err(ExtendedQueryError::unknown_type_oid(oid));
    }
    crate::sql::pagination::validate_statement(parsed, Some(admitted.types()))
        .map_err(|error| ExtendedQueryError::cassie(&error))?;
    crate::sql::validate_wire_parse_parameter_slots(admitted.types(), controls)
        .map_err(|error| ExtendedQueryError::parameter_admission(error, controls))?;
    Ok(admitted)
}
