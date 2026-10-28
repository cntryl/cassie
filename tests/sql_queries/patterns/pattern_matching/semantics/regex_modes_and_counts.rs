#[test]
fn should_apply_the_last_conflicting_regex_mode_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-flag-precedence", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like('A', 'a', 'ic'), regexp_like('A', 'a', 'ci'), regexp_like('a\nb', 'a.b', 'ns'), regexp_like('a\nb', 'a.b', 'sn')",
    );

    // Assert
    assert_eq!(
        result.expect("last conflicting regular-expression mode flag").rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_apply_postgres_regex_tight_syntax_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-tight-syntax", &[]);

    // Act
    let result =
        fixture.execute("SELECT regexp_like('a b', 'a b', 't'), regexp_like('ab', 'a b', 't')");

    // Assert
    assert_eq!(
        result.expect("tight-syntax regex flag").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_expanded_syntax_function_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-expanded-flag", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('alpha', '^ a l p h a $', 'x')");

    // Assert
    assert_eq!(
        result.expect("expanded-syntax regex function flag").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_count_matches_with_postgres_regex_start_options() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count('ABCABCAXYaxy', 'A.'), regexp_count('ABCABCAXYaxy', 'A.', 1, 'i'), regexp_count('ABCABCAXYaxy', 'A.', 4, 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_count").rows(),
        &[vec![Value::Int64(3), Value::Int64(4), Value::Int64(3)]]
    );
}

#[test]
fn should_count_the_longest_alternative_before_advancing() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-leftmost-longest", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('aaa', 'a|aa')");

    // Assert
    assert_eq!(
        result.expect("leftmost-longest regexp_count matches").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_keep_leftmost_longest_count_selection_for_lazy_quantifiers() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-lazy-selection", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('aaa', 'a+?')");

    // Assert
    assert_eq!(
        result.expect("leftmost-longest lazy-quantifier count").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_case_fold_unicode_letters_in_regexp_like() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-unicode-case", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('é', 'É', 'i')");

    // Assert
    assert_eq!(
        result.expect("Unicode case-insensitive regexp_like").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_count_unicode_case_insensitive_regex_matches() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-unicode-case", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('Éé', 'é', 1, 'i')");

    // Assert
    assert_eq!(
        result
            .expect("Unicode case-insensitive regexp_count")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_match_unicode_letters_with_the_regex_word_class() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-unicode-word-class", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('猫', '\\w')");

    // Assert
    assert_eq!(
        result.expect("Unicode regular-expression word class").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_count_nonoverlapping_zero_width_regex_matches() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-zero-width", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('aaa', 'a*')");

    // Assert
    assert_eq!(
        result.expect("non-overlapping zero-width regexp_count matches").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_count_regex_matches_from_a_one_based_unicode_character_start() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-unicode-start", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('éaé', 'é', 2)");

    // Assert
    assert_eq!(
        result.expect("regexp_count Unicode character start").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_count_an_empty_match_at_the_end_position() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-empty-at-end", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('abc', '', 4)");

    // Assert
    assert_eq!(
        result.expect("empty match at end position").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_count_empty_input_position_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-empty-input-boundaries", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count('', ''), regexp_count('', 'a'), regexp_count('', '', 1), regexp_count('', '', 2), regexp_count('', '', 2147483647)",
    );

    // Assert
    assert_eq!(
        result.expect("empty regexp_count input boundaries").rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(0),
            Value::Int64(1),
            Value::Int64(0),
            Value::Int64(0),
        ]]
    );
}

#[test]
fn should_return_zero_for_maximum_regexp_count_start_on_nonempty_text() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-maximum-start", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count('abc', '', 2147483647), regexp_count('abc', 'a', 2147483647)",
    );

    // Assert
    assert_eq!(
        result.expect("maximum regexp_count start on nonempty text").rows(),
        &[vec![Value::Int64(0), Value::Int64(0)]]
    );
}

#[test]
fn should_return_zero_when_regexp_count_starts_beyond_the_end_position() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-beyond-end", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('abc', '', 5)");

    // Assert
    assert_eq!(
        result.expect("no matches beyond end position").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_propagate_null_inputs_through_selected_regex_functions() {
    // Arrange
    let fixture = sql_fixture("issue-799-selected-regex-null-inputs", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like(NULL::TEXT, 'x'), regexp_like('x', NULL::TEXT), regexp_like('x', 'x', NULL::TEXT), regexp_count(NULL::TEXT, 'x'), regexp_count('x', NULL::TEXT), regexp_count('abc', 'a', NULL::INT4), regexp_count('abc', 'a', 1, NULL::TEXT), regexp_count('abc', 'a', NULL::INT4, NULL::TEXT), regexp_like(string => 'x', pattern => 'x', flags => NULL::TEXT), regexp_count(string => 'abc', pattern => 'a', start => NULL::INT4), regexp_count(string => 'abc', pattern => 'a', start => 1, flags => NULL::TEXT)",
    );

    // Assert
    assert_eq!(
        result.expect("NULL regular-expression inputs").rows(),
        &[vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]]
    );
}

#[test]
fn should_report_2201b_for_invalid_regexp_like_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-pattern", &[]);

    // Act
    let error = fixture
        .execute("SELECT regexp_like('x', '[')")
        .expect_err("invalid pattern should fail");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
}

#[test]
fn should_report_2201b_for_invalid_regexp_like_flags() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-flag", &[]);

    // Act
    let error = fixture
        .execute("SELECT regexp_like('x', 'x', 'z')")
        .expect_err("invalid flags should fail");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
}

#[test]
fn should_reject_postgres_are_backreferences_outside_the_selected_regex_profile() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-are-boundary", &[]);

    // Act
    let error = fixture
        .execute("SELECT regexp_like('abab', '^(ab)\\1$')")
        .expect_err("backreferences are outside the selected regex profile");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
}

#[test]
fn should_reject_nonpositive_regexp_count_start_positions() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-invalid-start", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('x', 'x', 0)");

    // Assert
    assert!(matches!(
        result.expect_err("zero start position should fail"),
        CassieError::InvalidParameterValue(_)
    ));
}

