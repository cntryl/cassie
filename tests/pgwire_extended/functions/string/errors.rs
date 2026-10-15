#[test]
fn should_report_invalid_parameter_value_for_zero_split_part_index() {
    // Arrange
    let batches = run_script(
        "issue-803-split-part-zero-index-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b', ',', 1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b', ',', 0)",
            )]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("a".to_owned())]]);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22023"));
}

#[test]
fn should_report_unsupported_utf8_to_ascii_conversion_with_0a000() {
    // Arrange
    let batches = run_script(
        "issue-803-to-ascii-utf8-sqlstate",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT to_ascii('abc')",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
}
