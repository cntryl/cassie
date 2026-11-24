#[test]
fn should_replace_at_unicode_character_offset_with_regexp_replace() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-character-offsets", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_replace('Aé🙂Z', '.', 'x', 2, 2, '')");

    // Assert
    assert_eq!(
        result.expect("regexp_replace Unicode character offset").rows(),
        &[vec![Value::String("AéxZ".to_owned())]]
    );
}

#[test]
fn should_replace_the_first_or_all_regex_matches_with_captured_text() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace('foobarbaz', 'b(..)', 'X\\1Y'), regexp_replace('foobarbaz', 'b(..)', 'X\\1Y', 'g'), regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 0, 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_replace variants").rows(),
        &[vec![
            Value::String("fooXarYbaz".to_owned()),
            Value::String("fooXarYXazY".to_owned()),
            Value::String("X PXstgrXSQL fXnctXXn".to_owned()),
        ]]
    );
}

#[test]
fn should_expand_regexp_replace_substitution_tokens() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-substitution-tokens", &[]);

    // Act
    let result = fixture.execute(
        r"SELECT regexp_replace('abc', 'b', 'X\&Y'), regexp_replace('abc', 'b', 'X\\Y'), regexp_replace('abc', 'b', 'X\qY'), regexp_replace('abc', 'b', 'X\0Y'), regexp_replace('abc', '(b)?c', 'X\2Y'), regexp_replace('abc', '(b)', 'X\10Y'), regexp_replace('abc', 'b', 'X\')",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_replace substitution tokens").rows(),
        &[vec![
            Value::String("aXbYc".to_owned()),
            Value::String(r"aX\Yc".to_owned()),
            Value::String(r"aX\qYc".to_owned()),
            Value::String(r"aX\0Yc".to_owned()),
            Value::String("aXY".to_owned()),
            Value::String("aXb0Yc".to_owned()),
            Value::String(r"aX\c".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_regexp_replace_empty_match_boundary_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-empty-match-boundaries", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace('abc', '', 'X', 4), regexp_replace('abc', '', 'X', 5), regexp_replace('abc', '', 'X', 1, 0)",
    );

    // Assert
    assert_eq!(
        result.expect("empty regexp_replace matches").rows(),
        &[vec![
            Value::String("abcX".to_owned()),
            Value::String("abc".to_owned()),
            Value::String("XaXbXcX".to_owned()),
        ]]
    );
}

#[test]
fn should_choose_the_longest_regexp_replace_capture() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-leftmost-longest", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_replace('aaa', '(a|aa)', '<\\1>', 1, 0)");

    // Assert
    assert_eq!(
        result.expect("leftmost-longest regexp_replace captures").rows(),
        &[vec![Value::String("<aa><a>".to_owned())]]
    );
}

#[test]
fn should_use_regexp_replace_start_occurrence_overloads() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-overloads", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace('abcabc', 'a', 'X', 'g'), regexp_replace('abcabc', 'a', 'X', 2), regexp_replace('abcabc', 'a', 'X', 1, 0), regexp_replace('abcabc', 'a', 'X', 1, 2), regexp_replace('abcabc', 'a', 'X', 1, 2, 'g'), regexp_replace('💡aéa', 'a', 'X', 2)",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_replace overloads").rows(),
        &[vec![
            Value::String("XbcXbc".to_owned()),
            Value::String("abcXbc".to_owned()),
            Value::String("XbcXbc".to_owned()),
            Value::String("abcXbc".to_owned()),
            Value::String("abcXbc".to_owned()),
            Value::String("💡Xéa".to_owned()),
        ]]
    );
}

#[test]
fn should_accept_short_named_regexp_replace_overloads() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-named-overloads", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace(string => 'abcabc', pattern => 'a', replacement => 'X', flags => 'g'), regexp_replace(string => 'abcabc', pattern => 'a', replacement => 'X', start => 2), regexp_replace(string => 'abcabc', pattern => 'a', replacement => 'X', start => 1, \"N\" => 2)",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_replace overloads").rows(),
        &[vec![
            Value::String("XbcXbc".to_owned()),
            Value::String("abcXbc".to_owned()),
            Value::String("abcXbc".to_owned()),
        ]]
    );
}

#[test]
fn should_report_invalid_regexp_replace_input_errors() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-errors", &[]);
    let cases = [
        ("SELECT regexp_replace('x', '[', 'y')", "2201B"),
        ("SELECT regexp_replace('x', 'x', 'y', 'z')", "2201B"),
        ("SELECT regexp_replace('x', 'x', 'y', 0)", "22023"),
        ("SELECT regexp_replace('x', 'x', 'y', 1, -1)", "22023"),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected_state), result) in cases.iter().zip(results) {
        let error = result.expect_err("invalid regexp_replace input should fail");
        let has_expected_state = match *expected_state {
            "2201B" => matches!(
                error,
                CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
            ),
            "22023" => matches!(error, CassieError::InvalidParameterValue(_)),
            _ => false,
        };
        assert!(has_expected_state, "{sql}: unexpected error {error:?}");
    }
}

#[test]
fn should_enforce_regexp_replace_output_memory_limit() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_memory_budget_bytes = 16 * 1024;
    let fixture = sql_fixture_with_config("issue-799-regexp-replace-memory-limit", &[], config);
    let memory_before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT regexp_replace($1, $2, $3, $4)",
        vec![
            Value::String("a".repeat(32)),
            Value::String("a".to_owned()),
            Value::String("x".repeat(1024)),
            Value::String("g".to_owned()),
        ],
    );

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "regexp_replace output beyond the query memory budget must fail: {result:?}"
    );
    assert_eq!(current_accounted_memory_bytes(&fixture), memory_before);
    assert_eq!(
        fixture.execute("SELECT 1::BIGINT").expect("query after replacement admission").rows(),
        &[vec![Value::Int64(1)]]
    );
}

