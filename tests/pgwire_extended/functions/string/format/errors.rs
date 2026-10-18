#[test]
fn should_report_invalid_parameter_value_for_unknown_format_conversion() {
    // Arrange
    let batches = run_script(
        "issue-803-format-unknown-conversion-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%s', 'valid') AS formatted",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%q', 'x') AS malformed_conversion",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("valid".to_owned())]]);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22023"));
    assert_eq!(wire::data_rows(invalid), Vec::<Vec<Option<String>>>::new());
    let fields = error_fields(invalid)
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    assert!(
        fields.iter().any(|(field, value)| {
            *field == 'M' && value == "unrecognized format() type specifier \"q\""
        }),
        "invalid format conversion must reach format()'s argument-validation path: {fields:?}"
    );
    assert!(
        fields
            .iter()
            .any(|(field, value)| { *field == 'H' && value == "For a single \"%\" use \"%%\"." }),
        "PostgreSQL 18.6 format() hint: {fields:?}"
    );
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}
