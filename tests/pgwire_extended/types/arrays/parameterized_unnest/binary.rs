#[test]
fn should_expand_binary_bound_array_with_null_element_to_binary_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        PARAMETERIZED_UNNEST_SQL,
        Some((1_007, 1, Some(NONDEFAULT_BOUND_ARRAY_BINARY))),
        1,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-808-parameterized-unnest-binary-bind",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_ordinality_descriptors(frames, 1);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![
            binary_result_row(Some(7), 1),
            binary_result_row(None, 2),
            binary_result_row(Some(9), 3),
        ]
    );
}
