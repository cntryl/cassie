#[test]
fn should_report_postgres_collation_conflicts_with_sqlstate_42p21() {
    // Arrange
    let batches = run_script(
        "issue-796-collation-conflict-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ('a' COLLATE \"C\") < ('b' COLLATE \"POSIX\")",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42P21"));
}

#[test]
fn should_reject_unparenthesized_parameter_arithmetic_in_offset_with_syntax_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-800-unparenthesized-offset-arithmetic",
        vec![Step::ParseFlush(wire::parse_frame(
            "bad_offset",
            "SELECT 1 OFFSET $1 + 1 ROWS",
        ))],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42601"));
    let error = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("unparenthesized OFFSET Parse error");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, value)| value.as_str()),
        Some("syntax error at or near \"ROWS\"")
    );
}

#[test]
fn should_report_malformed_unicode_identifier_escape_with_syntax_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-796-malformed-unicode-identifier",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT U&\"bad!00G0\" UESCAPE '!'",
        )])],
    );

    // Act
    let result = &batches[0];
    let error = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("malformed Unicode identifier ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42601"));
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, value)| value.as_str()),
        Some("invalid Unicode escape")
    );
}

#[test]
fn should_report_unsupported_comparison_for_empty_rows_with_0a000() {
    // Arrange
    let batches = run_script(
        "issue-800-empty-row-equality-sqlstate",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROW() = ROW()",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
}

#[test]
fn should_report_invalid_table_sample_percentages_with_2202h() {
    // Arrange
    let batches = run_script(
        "issue-800-tablesample-range-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_error_rows (id INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_800_sample_error_rows VALUES (1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_error_rows TABLESAMPLE BERNOULLI (-1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_error_rows TABLESAMPLE SYSTEM (101)",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let inserted = &batches[1];
    let negative_rate = &batches[2];
    let overfull_rate = &batches[3];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(wire::error_code(inserted), None);
    assert_eq!(wire::error_code(negative_rate).as_deref(), Some("2202H"));
    assert_eq!(wire::error_code(overfull_rate).as_deref(), Some("2202H"));
}

#[test]
fn should_report_values_using_row_arity_sqlstates() {
    // Arrange
    let cases = [
        ("VALUES (1, 2), (3)", "42601"),
        ("SELECT * FROM (VALUES (1, 2)) AS v(x, y, z)", "42P10"),
        ("SELECT * FROM (VALUES (1), (TRUE)) AS v(value)", "42804"),
        ("SELECT ROW(1, 2) = ROW(1)", "42601"),
        (
            "SELECT a.id FROM (VALUES (1)) AS a(id) JOIN (VALUES (1)) AS b(id) USING (missing)",
            "42703",
        ),
        (
            "SELECT * FROM (VALUES (1)) AS a(id) JOIN (VALUES (1)) AS b(id) USING (id, id)",
            "42701",
        ),
    ];
    let steps = cases
        .iter()
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("issue-800-values-using-row-errors", steps);
    let actual = batches
        .iter()
        .map(|batch| wire::error_code(batch))
        .collect::<Vec<_>>();
    let expected = cases
        .iter()
        .map(|(_, state)| Some((*state).to_owned()))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(actual, expected);
}
