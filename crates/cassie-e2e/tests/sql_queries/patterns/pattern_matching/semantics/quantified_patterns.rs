#[test]
fn should_match_quantified_like_regex_operator_forms() {
    // Arrange
    let fixture = sql_fixture("issue-799-quantified-pattern-modes", &[]);

    // Act
    let result = fixture.execute("SELECT 'abc' LIKE ANY(ARRAY['z%','a%']), 'ABC' ILIKE SOME(ARRAY['z%','a%']), 'abc' NOT LIKE ANY(ARRAY['a%','z%']), 'abc' NOT LIKE ALL(ARRAY['a%','z%']), 'abc' ~ ANY(ARRAY['^z','^a']), 'ABC' ~* ANY(ARRAY['^a']), 'abc' !~ ALL(ARRAY['^z','^q']), 'ABC' !~~* ALL(ARRAY['z%','q%']), 'abc' ~~ ANY(ARRAY['a%']), 'ABC' ~~* ANY(ARRAY['a%'])");

    // Assert
    assert_eq!(
        result
            .expect("quantified keyword and operator modes")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_preserve_quantified_pattern_null_empty_identities() {
    // Arrange
    let fixture = sql_fixture("issue-799-quantified-pattern-nulls", &[]);

    // Act
    let result = fixture.execute("SELECT NULL LIKE ANY(ARRAY[]::TEXT[]), NULL LIKE ALL(ARRAY[]::TEXT[]), 'abc' ILIKE ANY(NULL::TEXT[]), 'abc' LIKE ANY(ARRAY[NULL,'z%']::TEXT[]), 'abc' ~ ANY(ARRAY['^a',NULL]::TEXT[]), 'abc' ~ ALL(ARRAY['^z',NULL]::TEXT[]), NULL ~ ANY(ARRAY['[']::TEXT[])");

    // Assert
    assert_eq!(
        result
            .expect("quantified strict matching with empty identity")
            .rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Null,
            Value::Null,
            Value::Bool(true),
            Value::Bool(false),
            Value::Null
        ]]
    );
}

#[test]
fn should_stop_quantified_regex_after_a_decisive_element() {
    // Arrange
    let fixture = sql_fixture("issue-799-quantified-pattern-short-circuit", &[]);

    // Act
    let result = fixture.execute("SELECT 'abc' ~ ANY(ARRAY['^a','[']), 'abc' ~ ALL(ARRAY['^z','[']), 'abc' LIKE ANY(SELECT 'a%'::TEXT)");

    // Assert
    assert_eq!(
        result.expect("ordered pattern evaluation").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_reject_quantified_pattern_escape_clauses() {
    // Arrange
    let fixture = sql_fixture("issue-799-quantified-pattern-invalid-syntax", &[]);
    let sql = [
        "SELECT 'x' LIKE ANY(ARRAY['x']) ESCAPE '!'",
        "SELECT 'x' SIMILAR TO ANY(ARRAY['x'])",
        "SELECT __cassie_quantified_pattern('x', ARRAY['x'], 'like', FALSE, FALSE)",
    ];

    // Act
    let results = sql.map(|sql| fixture.execute(sql));

    // Assert
    for result in results {
        assert!(result.is_err());
    }
}

#[test]
fn should_reject_direct_internal_quantified_helper_calls() {
    // Arrange
    let fixture = sql_fixture("issue-799-private-helper-call", &[]);
    let names = [
        "__cassie_quantified_pattern",
        "__CASSIE_QUANTIFIED_PATTERN",
        "pg_catalog.__cassie_quantified_pattern",
    ];

    // Act
    let results = names.map(|name| {
        fixture.execute(&format!(
            "SELECT {name}('x', ARRAY['x'], 'like', FALSE, FALSE)"
        ))
    });

    // Assert
    for result in results {
        assert!(result.is_err());
    }
}
