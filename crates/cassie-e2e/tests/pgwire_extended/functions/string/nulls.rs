#[test]
fn should_report_null_value_not_allowed_for_null_identifier_format() {
    // Arrange
    let batches = run_script(
        "issue-803-format-null-identifier-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', 'identifier')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', NULL::TEXT)",
            )]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(
        wire::data_rows(valid),
        vec![vec![Some("identifier".to_owned())]]
    );
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22004"));
}
