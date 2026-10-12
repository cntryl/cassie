//! Private Parse resource mapping also covers the admitted semantic-error frame.

use std::time::Instant;

use super::ExtendedQueryError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::sql::{ParameterAdmissionError, ParameterSemanticDiagnostic};

#[test]
fn should_preserve_parse_out_of_memory_when_semantic_error_admission_is_denied() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let ample = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 65_536,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let diagnostic = ParameterSemanticDiagnostic::Indeterminate { slot: 0 };

    // Act
    let error = ExtendedQueryError::parameter_admission(
        ParameterAdmissionError::Semantic(diagnostic),
        &controls,
    );
    let admitted = ExtendedQueryError::parameter_admission(
        ParameterAdmissionError::Semantic(diagnostic),
        &ample,
    );
    let retained = ample.current_query_memory_bytes();
    let semantic_fields_match = admitted.pg_error.code == "42P08"
        && admitted.pg_error.message == "could not determine data type of parameter $1"
        && admitted.pg_error.detail.is_none();
    let admitted_frame_present = admitted
        .pg_error
        .encoded_semantic_frame()
        .is_some_and(|frame| {
            frame.first() == Some(&b'E') && frame.windows(7).any(|field| field == b"C42P08\0")
        });
    let admitted_protocol_error = admitted.record_protocol_error;
    drop(admitted);
    let released = ample.current_query_memory_bytes();

    // Assert
    assert_eq!(error.pg_error.code, "53200");
    assert!(!error.record_protocol_error);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(controls.peak_query_memory_bytes() <= 1);
    assert!(semantic_fields_match);
    assert!(admitted_frame_present);
    assert!(!admitted_protocol_error);
    assert!(retained > 0);
    assert_eq!(released, 0);
}
