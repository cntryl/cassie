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

#[test]
fn should_match_postgresql_format_argument_contract_for_zero_position() {
    // Arrange
    let batches = run_script(
        "issue-803-format-zero-position-contract",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT format('%0$s', 'x')",
        )])],
    );

    // Act
    let invalid = &batches[0];

    // Assert
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22023"));
    let fields = format_error_fields(invalid);
    assert!(fields.iter().any(|(field, value)| {
        *field == 'M'
            && value == "format specifies argument 0, but arguments are numbered from 1"
    }));
}

#[test]
fn should_match_postgresql_format_argument_contract_for_invalid_width() {
    // Arrange
    let batches = run_script(
        "issue-803-format-invalid-width-contract",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT format('%*s', 'x', 'y')",
        )])],
    );

    // Act
    let invalid = &batches[0];

    // Assert
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22P02"));
    let fields = format_error_fields(invalid);
    assert!(fields.iter().any(|(field, value)| {
        *field == 'M' && value == "invalid input syntax for type integer: \"x\""
    }));
}

#[test]
fn should_match_postgresql_format_argument_contract_for_width_overflow() {
    // Arrange
    let batches = run_script(
        "issue-803-format-width-overflow-contract",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT format('%*s', 2147483648, 'y')",
        )])],
    );

    // Act
    let invalid = &batches[0];

    // Assert
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22003"));
    let fields = format_error_fields(invalid);
    assert!(fields.iter().any(|(field, value)| {
        *field == 'M' && value == "value \"2147483648\" is out of range for type integer"
    }));
}

#[test]
fn should_accept_int4_minimum_dynamic_width_before_null_identifier_error() {
    // Arrange
    let batches = run_script(
        "issue-803-format-int4-minimum-width",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT format('%*I', -2147483648, NULL)",
        )])],
    );

    // Act
    let invalid = &batches[0];

    // Assert
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22004"));
    let fields = format_error_fields(invalid);
    assert!(fields.iter().any(|(field, value)| {
        *field == 'M' && value == "null values cannot be formatted as an SQL identifier"
    }));
}

#[test]
fn should_reject_dynamic_format_width_below_int4_minimum() {
    // Arrange
    let batches = run_script(
        "issue-803-format-below-int4-minimum-width",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT format('%*I', -2147483649, NULL)",
        )])],
    );

    // Act
    let invalid = &batches[0];

    // Assert
    assert_eq!(wire::error_code(invalid).as_deref(), Some("22003"));
    let fields = format_error_fields(invalid);
    assert!(fields.iter().any(|(field, value)| {
        *field == 'M' && value == "value \"-2147483649\" is out of range for type integer"
    }));
}

#[test]
fn should_match_postgresql_format_argument_contract_for_unterminated_specifiers() {
    // Arrange
    let queries = [
        "SELECT format('%')",
        "SELECT format('%1$')",
        "SELECT format('%*')",
    ];
    let steps = queries
        .iter()
        .map(|query| Step::ThroughReady(vec![wire::simple_query_frame(query)]))
        .collect();

    // Act
    let batches = run_script("issue-803-format-unterminated-contract", steps);

    // Assert
    assert_eq!(batches.len(), queries.len());
    for invalid in &batches {
        assert_eq!(wire::error_code(invalid).as_deref(), Some("22023"));
        let fields = format_error_fields(invalid);
        assert!(fields.iter().any(|(field, value)| {
            *field == 'M' && value == "unterminated format() type specifier"
        }));
        assert!(fields.iter().any(|(field, value)| {
            *field == 'H' && value == "For a single \"%\" use \"%%\"."
        }));
    }
}

fn format_error_fields(frames: &[(u8, Vec<u8>)]) -> Vec<(char, String)> {
    error_fields(frames).into_iter().flatten().collect()
}
