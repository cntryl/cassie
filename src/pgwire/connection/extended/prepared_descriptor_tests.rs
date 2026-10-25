use super::*;

#[test]
fn should_reject_prepared_descriptor_retention_when_connection_query_budget_is_exhausted() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..crate::config::CassieRuntimeLimits::default()
    };
    let state = SessionState::new(&limits);
    let columns = vec![ColumnMeta::text("value")];

    // Act
    let result = retain_prepared_result_descriptor(&state, columns, 0);

    // Assert
    let error = result.expect_err("descriptor retention must fit the connection query budget");
    assert_eq!(error.pg_error.code, "54000");
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_prepared_descriptor_admission_when_its_last_owner_is_dropped() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits::default();
    let state = SessionState::new(&limits);

    // Act
    let descriptor = retain_prepared_result_descriptor(
        &state,
        vec![ColumnMeta::text("value"), ColumnMeta::text("other")],
        0,
    )
    .expect("descriptor should fit the connection query budget");

    // Assert
    assert!(state.portal_memory_controls.current_query_memory_bytes() > 0);
    drop(descriptor);
    assert_eq!(state.portal_memory_controls.current_query_memory_bytes(), 0);
}
