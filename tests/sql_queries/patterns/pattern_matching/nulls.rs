#[test]
fn should_preserve_null_results_for_ilike_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike-null", &[]);

    // Act
    let result = fixture.execute("SELECT NULL ILIKE 'x%', NULL NOT ILIKE 'x%'");

    // Assert
    assert_eq!(
        result.expect("NULL ILIKE predicates").rows(),
        &[vec![Value::Null, Value::Null]]
    );
}

#[test]
fn should_disable_like_escaping_for_an_empty_escape_clause() {
    // Arrange
    let fixture = sql_fixture("issue-799-like-empty-escape", &[]);

    // Act
    let result = fixture.execute("SELECT 'a!xb' LIKE 'a!_b' ESCAPE ''");

    // Assert
    assert_eq!(
        result.expect("LIKE with disabled escaping").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_not_like_without_matching_null_values() {
    // Arrange
    let fixture = sql_fixture("issue-799-not-like", &[]);

    // Act
    let result = fixture.execute("SELECT 'abc' NOT LIKE 'a%', NULL NOT LIKE 'a%'");

    // Assert
    assert_eq!(
        result.expect("NOT LIKE predicates").rows(),
        &[vec![Value::Bool(false), Value::Null]]
    );
}

#[test]
fn should_evaluate_not_similar_to_with_sql_null_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-799-not-similar-to", &[]);

    // Act
    let result = fixture.execute("SELECT 'abc' NOT SIMILAR TO 'a%', NULL NOT SIMILAR TO 'a%'");

    // Assert
    assert_eq!(
        result.expect("NOT SIMILAR TO predicates").rows(),
        &[vec![Value::Bool(false), Value::Null]]
    );
}

#[test]
fn should_preserve_null_results_for_regex_operators() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-null", &[]);

    // Act
    let result = fixture.execute("SELECT NULL ~ 'x', 'x' !~ NULL, NULL ~* 'x', 'x' !~* NULL");

    // Assert
    assert_eq!(
        result.expect("NULL regular-expression operators").rows(),
        &[vec![Value::Null, Value::Null, Value::Null, Value::Null]]
    );
}

#[test]
fn should_preserve_null_inputs_for_regexp_replace() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-null-inputs", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace(NULL, 'a', 'b'), regexp_replace('a', NULL, 'b'), regexp_replace('a', 'a', NULL), regexp_replace('a', 'a', 'b', NULL::TEXT), regexp_replace('a', 'a', 'b', NULL::INT), regexp_replace('a', 'a', 'b', 1, NULL::INT), regexp_replace('a', 'a', 'b', 1, 1, NULL::TEXT)",
    );

    // Assert
    assert_eq!(
        result.expect("NULL regexp_replace inputs").rows(),
        &[vec![
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
