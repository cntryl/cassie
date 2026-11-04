#[test]
fn should_match_posix_patterns_without_implicit_anchors() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operators", &[]);

    // Act
    let result = fixture
        .execute("SELECT 'abcd' ~ 'bc', 'abcd' ~ 'a.c', 'abcd' ~ '^(b|c)', 'abcd' !~ '^(b|c)'");

    // Assert
    assert_eq!(
        result.expect("POSIX regular-expression operators").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_ignore_nonexpression_regex_tokens() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operator-token-boundaries", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'a~b' = 'a~b', 'alpha' /* ~ ignored */ ~ 'ph'",
    );

    // Assert
    assert_eq!(
        result.expect("regex tokens in strings and comments").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_regex_comparison_precedence() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operator-comparison-precedence", &[]);

    // Act
    let result = fixture.execute("SELECT 'alpha' ~ 'ph' = true");

    // Assert
    assert_eq!(
        result.expect("regex comparison precedence").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_report_2201b_for_invalid_regex_operator_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operator-invalid-pattern", &[]);

    // Act
    let error = fixture
        .execute("SELECT 'x' ~ '['")
        .expect_err("invalid regular-expression operator pattern should fail");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
}

#[test]
fn should_filter_stored_text_rows_by_posix_regex_predicate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-799-regex-table-predicate",
        &[
            "CREATE TABLE issue_799_regex_rows (body TEXT)",
            "INSERT INTO issue_799_regex_rows VALUES ('fat cat'), ('fat dog'), ('the cat')",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT body FROM issue_799_regex_rows WHERE body ~ 'cat' ORDER BY body");

    // Assert
    assert_eq!(
        result.expect("regex predicate over stored text").rows(),
        &[
            vec![Value::String("fat cat".to_owned())],
            vec![Value::String("the cat".to_owned())],
        ]
    );
}

#[test]
fn should_apply_case_insensitive_posix_regex_operator_spellings() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-case-insensitive", &[]);

    // Act
    let result = fixture.execute("SELECT 'Thomas' ~* 'T.*MA', 'Thomas' !~* 't.*ma'");

    // Assert
    assert_eq!(
        result.expect("case-insensitive regex operators").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_match_newlines_with_the_default_regular_expression_mode() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-default-newlines", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'a\nb' ~ 'a.b', 'a\nb' ~* 'A.B', 'a\nb' !~ 'a.b', regexp_like('a\nb', 'a.b'), regexp_like('a\nb', 'a.b', ''), regexp_count('a\nb', '.'), regexp_replace('a\nb', '.', 'x', 'g')",
    );

    // Assert
    assert_eq!(
        result.expect("default regex mode includes newline characters").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int64(3),
            Value::String("xxx".to_owned()),
        ]]
    );
}

#[test]
fn should_exclude_newlines_from_negated_classes_in_sensitive_regex_modes() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-negated-class-newlines", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like('\n', '[^x]'), regexp_like('\n', '[^x]', 'n'), regexp_like('\n', '[^x]', 'm'), regexp_like('\n', '[^x]', 'p'), regexp_like('\n', '[^x]', 'w'), regexp_like('\n', '[^x]', 's'), regexp_like('猫', '[^x]', 'n'), regexp_count('a\nb', '[^x]', 1, 'n'), regexp_replace('a\nb', '[^x]', 'X', 1, 0, 'p')",
    );

    // Assert
    assert_eq!(
        result.expect("newline-sensitive negated classes").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int64(2),
            Value::String("X\nX".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_regex_class_quoting_while_excluding_newlines() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-negated-class-quoting", &[]);

    // Act
    let result = fixture.execute(
        r"SELECT regexp_like('a', '[^]]', 'n'), regexp_like(']', '[^]]', 'n'), regexp_like('[^x]', '\[\^x\]', 'n'), regexp_like('[^x]', '[^x]', 'q'), regexp_like('猫', '[^犬]', 'n')",
    );

    // Assert
    assert_eq!(
        result.expect("quoted and Unicode negated-class boundaries").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_reject_incompatible_modes_for_quoted_regex_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-quoted-newline-modes", &[]);
    let flags = ["qn", "nq", "qp", "qw", "qx"];

    // Act
    let results = flags.map(|flags| {
        fixture.execute(&format!("SELECT regexp_like('[^x]', '[^x]', '{flags}')"))
    });

    // Assert
    for (flags, result) in flags.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Diagnostic(ref diagnostic)) if diagnostic.sql_state == "2201B"),
            "quoted regex with {flags} must reject incompatible newline semantics: {result:?}"
        );
    }
}

#[test]
fn should_treat_regex_directors_as_literal_text_in_quoted_mode() {
    // Arrange
    let fixture = sql_fixture("issue-799-quoted-regex-directors", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like('***=a.b', '***=a.b', 'q'), regexp_like('a.b', '***=a.b', 'q'), regexp_like('***:^a$', '***:^a$', 'q'), regexp_like('a b', '***=a b', 'x'), regexp_like('ab', '***=a b', 'x'), regexp_like('aaa', '***:^a+$', 'b')",
    );

    // Assert
    assert_eq!(
        result.expect("quoted syntax keeps director prefixes literal").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_accept_quoted_regex_modes_after_conflicting_flags_are_reset() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-quoted-mode-reset", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like('\n', '\n', 'qs'), regexp_like('\n', '\n', 'qns'), regexp_like('a b', 'a b', 'qxt')",
    );

    // Assert
    assert_eq!(
        result.expect("last mode flag restores compatible quoted syntax").rows(),
        &[vec![Value::Bool(true), Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_match_postgres_are_option_director_forms() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-options-directors", &[]);
    let cases = [
        ("SELECT 'ABC' ~ '(?i)^abc$'", true),
        ("SELECT 'alpha' ~ '(?x)^ a l p h a $'", true),
        ("SELECT 'a.b' ~ '***=a.b'", true),
        ("SELECT 'alpha' ~ '***:^alpha$'", true),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected), result) in cases.iter().zip(results) {
        assert_eq!(
            result.expect("PostgreSQL ARE option or director").rows(),
            &[vec![Value::Bool(*expected)]],
            "{sql}"
        );
    }
}

#[test]
fn should_match_posix_bre_ere_modes() {
    // Arrange
    let fixture = sql_fixture("issue-799-basic-extended-regex-modes", &[]);

    // Act
    let result = fixture
        .execute("SELECT regexp_like('a+b', '^a+b$', 'b'), regexp_like('aaab', '^a+b$', 'e')");

    // Assert
    assert_eq!(
        result.expect("BRE and ERE modes").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_apply_postgres_regex_quoted_case_flags() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-quoted-flags", &[]);

    // Act
    let result = fixture
        .execute("SELECT regexp_like('^a+$', '^a+$', 'q'), regexp_like('Thomas', 'thomas', 'c')");

    // Assert
    assert_eq!(
        result
            .expect("quoted and case-sensitive regex flags")
            .rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_newline_sensitive_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-newline-sensitive", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', 'a.b', 'n')");

    // Assert
    assert_eq!(
        result.expect("newline-sensitive regex flag").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_multiline_alias_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-multiline-alias", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', 'a.b', 'm')");

    // Assert
    assert_eq!(
        result.expect("historical multiline regex flag").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_non_newline_sensitive_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-non-newline-sensitive", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', 'a.b', 's')");

    // Assert
    assert_eq!(
        result.expect("non-newline-sensitive regex flag").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_apply_postgres_regex_partial_newline_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-partial-newline", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', '^b$', 'p')");

    // Assert
    assert_eq!(
        result.expect("partial newline-sensitive regex flag").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_inverse_partial_newline_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-inverse-partial-newline", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', '^b$', 'w')");

    // Assert
    assert_eq!(
        result
            .expect("inverse partial newline-sensitive regex flag")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_match_postgres_regex_newline_flag_matrix() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-newline-flag-matrix", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like('a\nb', '^b$', 'n'), regexp_like('a\nb', '^b$', 'm'), regexp_like('a\nb', '^b$', 'p'), regexp_like('a\nb', '^b$', 'w'), regexp_like('a\nb', '^b$', 's'), regexp_like('a\nb', 'a.b', 'n'), regexp_like('a\nb', 'a.b', 'm'), regexp_like('a\nb', 'a.b', 'p'), regexp_like('a\nb', 'a.b', 'w'), regexp_like('a\nb', 'a.b', 's')",
    );

    // Assert
    assert_eq!(
        result.expect("PostgreSQL newline-sensitive anchor and dot behavior").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

