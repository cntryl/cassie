//! Admitted parameter error conversion.
use super::{CassieError, ExtendedQueryError, PgWireError, PgWireSeverity};
impl ExtendedQueryError {
    pub(super) fn parameter_admission(
        error: crate::sql::ParameterAdmissionError,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Self {
        use crate::sql::ParameterAdmissionError;

        let (sqlstate, message) = match error {
            ParameterAdmissionError::Memory(message) => ("53200", message),
            ParameterAdmissionError::InvalidParam(message) => ("42P02", message),
            ParameterAdmissionError::Cancelled => ("57014", "query cancelled".to_string()),
            ParameterAdmissionError::Deadline => ("57014", "query deadline exceeded".to_string()),
            ParameterAdmissionError::InvalidBooleanLiteral {
                input,
                _reservation: reservation,
            } => {
                let result = Self::admitted_parameter_error(PgWireError::invalid_boolean_input(
                    &input, controls,
                ));
                drop(input);
                drop(reservation);
                return result;
            }
            ParameterAdmissionError::LegacyConflict(message) => {
                return Self::cassie(&CassieError::Planner(message));
            }
            ParameterAdmissionError::InvalidScalarLiteral {
                input,
                diagnostic,
                _reservation: reservation,
            } => {
                return Self::scalar_literal_error(input, diagnostic, reservation, controls);
            }
            ParameterAdmissionError::NonAggregateModifier {
                message,
                _reservation: reservation,
            } => {
                let result = Self::admitted_parameter_error(PgWireError::nonaggregate_modifier(
                    &message, controls,
                ));
                drop(message);
                drop(reservation);
                return result;
            }
            ParameterAdmissionError::ByteaInput(error) => {
                return Self::admitted_parameter_error(PgWireError::function_diagnostic(
                    error.sql_state(),
                    &error.to_string(),
                    controls,
                ));
            }
            ParameterAdmissionError::FunctionDiagnostic {
                sqlstate,
                message,
                _reservation: reservation,
            } => {
                let result = Self::admitted_parameter_error(PgWireError::function_diagnostic(
                    sqlstate, &message, controls,
                ));
                drop(message);
                drop(reservation);
                return result;
            }
            ParameterAdmissionError::Semantic(diagnostic) => {
                return Self::admitted_parameter_error(PgWireError::parameter_semantic(
                    diagnostic, controls,
                ));
            }
        };
        Self {
            pg_error: Box::new(PgWireError::new(PgWireSeverity::Error, sqlstate, message)),
            record_protocol_error: false,
        }
    }
}
