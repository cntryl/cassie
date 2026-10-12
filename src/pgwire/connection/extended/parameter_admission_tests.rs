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
    let diagnostic = ParameterSemanticDiagnostic::Indeterminate { slot: 0 };

    // Act
    let error = ExtendedQueryError::parameter_admission(
        ParameterAdmissionError::Semantic(diagnostic),
        &controls,
    );

    // Assert
    assert_eq!(error.pg_error.code, "53200");
    assert!(!error.record_protocol_error);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(controls.peak_query_memory_bytes() <= 1);
}
