use super::decode_identity;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_fail_closed_for_a_legacy_row_descriptor_without_identity() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    // Act
    let id = decode_identity(br#"{"schema_version":1,"fields":[]}"#, &controls).expect("decode");
    // Assert
    assert_eq!(id, None);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_identity_decode_over_query_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let id = decode_identity(br#"{"relation_id":42}"#, &controls);
    // Assert
    assert!(id.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
