#[test]
fn should_resolve_an_unknown_array_concatenation_literal() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT ARRAY[1,2] || '{3,4}' AS combined", None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-array-unknown-concat-literal", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{1,2,3,4}".to_owned())]]
    );
}

#[test]
fn should_apply_postgresql_unknown_array_concatenation_resolution() {
    // Arrange
    let unknown_null = fixture::execute_cycle("SELECT (ARRAY[1,2] || NULL)::TEXT", None, 0, true);
    let typed_null =
        fixture::execute_cycle("SELECT (ARRAY[1,2] || NULL::INTEGER)::TEXT", None, 0, true);
    let unknown_scalar = fixture::execute_cycle("SELECT ARRAY[1,2] || '7'", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-807-array-concat-unknown-operands",
        &[],
        vec![unknown_null, typed_null, unknown_scalar],
    );

    // Assert
    for frames in [&batches[0], &batches[1]] {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    }
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{1,2}".to_owned())]]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("{1,2,NULL}".to_owned())]]
    );
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22P02"));
}
