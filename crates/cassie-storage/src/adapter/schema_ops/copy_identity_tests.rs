use super::decode_identity;
use crate::adapter::request_test_support::TestRequest;

#[test]
fn should_fail_closed_for_a_legacy_row_descriptor_without_identity() {
    // Arrange
    let controls = TestRequest::default();
    // Act
    let id = decode_identity(br#"{"schema_version":1,"fields":[]}"#, &controls.context())
        .expect("decode");
    // Assert
    assert_eq!(id, None);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_identity_decode_over_query_budget() {
    // Arrange
    let controls = TestRequest::with_budget(1);
    // Act
    let id = decode_identity(br#"{"relation_id":42}"#, &controls.context());
    // Assert
    assert!(id.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
