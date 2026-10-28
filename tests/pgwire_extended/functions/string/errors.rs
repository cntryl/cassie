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
