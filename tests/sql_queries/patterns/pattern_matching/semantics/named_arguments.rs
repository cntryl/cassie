#[test]
fn should_accept_named_arguments_for_regexp_like() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-named", &[]);

    // Act
    let result = fixture
        .execute("SELECT regexp_like(string => 'Hello World', pattern => 'world', flags => 'i')");

    // Assert
    assert_eq!(
        result.expect("named regexp_like arguments").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_replace() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace(string => 'A PostgreSQL function', pattern => 'a|e|i|o|u', replacement => 'X', start => 1, \"N\" => 3, flags => 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_replace arguments").rows(),
        &[vec![Value::String("A PostgrXSQL function".to_owned())]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_count() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count(string => 'ABCABCAXYaxy', pattern => 'A.', start => 1, flags => 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_count arguments").rows(),
        &[vec![Value::Int64(4)]]
    );
}

#[test]
fn should_evaluate_pg_catalog_qualified_regex_function_aliases() {
    // Arrange
    let fixture = sql_fixture("issue-799-qualified-regex-functions", &[]);

    // Act
    let result = fixture.execute(
        "SELECT pg_catalog.regexp_like('Thomas', 'thomas', 'i'), pg_catalog.regexp_count('ABCABC', 'A.', 1, 'i'), pg_catalog.regexp_like(string => 'Hello', pattern => 'hello', flags => 'i'), pg_catalog.regexp_count(string => 'ABCABC', pattern => 'A.', start => 1, flags => 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("qualified regular-expression function aliases").rows(),
        &[vec![
            Value::Bool(true),
            Value::Int64(2),
            Value::Bool(true),
            Value::Int64(2),
        ]]
    );
}

#[test]
fn should_reject_invalid_selected_regex_named_arguments() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-named-arguments", &[]);
    let cases = [
        (
            "SELECT regexp_like(string => 'x', pattern => 'x', pattern => 'y')",
            "duplicate named argument 'pattern'",
        ),
        (
            "SELECT regexp_like('x', pattern => 'x')",
            "cannot mix positional and named function arguments",
        ),
        (
            "SELECT regexp_like(string => 'x')",
            "requires named arguments 'string' and 'pattern'",
        ),
        (
            "SELECT regexp_count(string => 'x', pattern => 'x', mode => 'i')",
            "unknown named argument 'mode'",
        ),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected_message), result) in cases.iter().zip(results) {
        let error = result.expect_err("invalid named arguments should fail");
        match error {
            CassieError::InvalidQuery(message) => assert!(
                message.contains(expected_message),
                "{sql}: expected {expected_message:?}, got {message:?}"
            ),
            other => panic!("{sql}: expected a parser error, got {other:?}"),
        }
    }
}

#[test]
fn should_reject_unsupported_positional_regex_function_arities() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-positional-arities", &[]);
    let cases = [
        ("SELECT regexp_like('x')", "regexp_like"),
        ("SELECT regexp_like('x', 'x', 'i', 'extra')", "regexp_like"),
        ("SELECT regexp_count('x')", "regexp_count"),
        (
            "SELECT regexp_count('x', 'x', 1, 'i', 'extra')",
            "regexp_count",
        ),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, function), result) in cases.iter().zip(results) {
        let error = result.expect_err("unsupported arity should fail");
        match error {
            CassieError::Planner(message) => assert!(
                message.starts_with(&format!("function '{function}' expects "))
                    && message.contains(", got "),
                "{sql}: expected an invalid arity error, got {message:?}"
            ),
            other => panic!("{sql}: expected an invalid arity error, got {other:?}"),
        }
    }
}

