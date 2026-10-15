#[test]
fn should_parse_escape_string_constants_with_postgres_backslash_rules() {
    // Arrange
    let fixture = sql_fixture("issue-796-escape-string", &[]);

    // Act
    let result = fixture.execute(r"SELECT E'line\nfeed' AS value");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL escape string").rows(),
        &[vec![Value::String("line\nfeed".to_owned())]]
    );
}

#[test]
fn should_parse_unicode_escape_string_constant_forms() {
    // Arrange
    let fixture = sql_fixture("issue-796-unicode-escape-string", &[]);

    // Act
    let result = fixture.execute("SELECT U&'d!0061t' UESCAPE '!' AS value");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL Unicode escape string").rows(),
        &[vec![Value::String("dat".to_owned())]]
    );
}

#[test]
fn should_join_adjacent_string_constants_across_newlines() {
    // Arrange
    let fixture = sql_fixture("issue-796-adjacent-string-constants", &[]);

    // Act
    let result = fixture.execute("SELECT 'alpha'\n       'beta' AS value");

    // Assert
    assert_eq!(
        result.expect("newline-separated string literals").rows(),
        &[vec![Value::String("alphabeta".to_owned())]]
    );
}

#[test]
fn should_accept_a_postgres_type_name_string_literal() {
    // Arrange
    let fixture = sql_fixture("issue-796-generic-type-string-literal", &[]);

    // Act
    let result = fixture.execute("SELECT INTEGER '42' AS value");

    // Assert
    let result = result.expect("a type name may directly precede its string literal");
    assert_eq!(result.rows(), &[vec![Value::Int64(42)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_apply_postgres_arithmetic_before_string_concatenation() {
    // Arrange
    let fixture = sql_fixture("issue-796-arithmetic-concatenation-precedence", &[]);

    // Act
    let result =
        fixture.execute("SELECT 'a' || 1 + 2 AS text_right_add, 1 + 2 || 'b' AS add_left_text");

    // Assert
    let result = result.expect("arithmetic binds more tightly than concatenation");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("a3".to_owned()),
            Value::String("3b".to_owned()),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25]
    );
}
