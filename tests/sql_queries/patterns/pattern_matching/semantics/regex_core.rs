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

