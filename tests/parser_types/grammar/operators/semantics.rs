#[test]
fn should_apply_postgres_modulo_bitwise_or_operators() {
    // Arrange
    let label = "issue-796-modulo-and-bitwise-or";

    // Act
    let result = sql_fixture(label, &[]).execute("SELECT 5 % 2, 5 | 2");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL modulo and bitwise OR").rows(),
        &[vec![Value::Int64(1), Value::Int64(7)]]
    );
}

#[test]
fn should_resolve_schema_qualified_unary_operators() {
    // Arrange
    let fixture = sql_fixture("issue-796-qualified-unary-operator", &[]);

    // Act
    let result = fixture.execute("SELECT OPERATOR(pg_catalog.-) 2 AS value");

    // Assert
    assert_eq!(
        result.expect("qualified unary operator").rows(),
        &[vec![Value::Int64(-2)]]
    );
}

#[test]
fn should_parse_postgres_postfix_absolute_value_operator() {
    // Arrange
    let fixture = sql_fixture("issue-796-absolute-value-operator", &[]);

    // Act
    let result = fixture.execute("SELECT @ -7 AS value");

    // Assert
    assert_eq!(
        result.expect("prefix absolute-value operator").rows(),
        &[vec![Value::Int64(7)]]
    );
}
