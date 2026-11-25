#[test]
fn should_use_left_associative_postgres_power_with_float8_result() {
    // Arrange
    let fixture = sql_fixture("issue-796-bitwise-xor-precedence", &[]);

    // Act
    let result = fixture.execute("SELECT 2 ^ 3 ^ 2 AS value");

    // Assert
    let result = result.expect("left-associative floating-point exponentiation");
    assert_eq!(result.rows(), &[vec![Value::Float64(64.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}

#[test]
fn should_apply_postgres_unary_minus_precedence_around_power() {
    // Arrange
    let fixture = sql_fixture("issue-796-unary-power-precedence", &[]);

    // Act
    let result =
        fixture.execute("SELECT -2 ^ 2 AS unary_before_power, -(2::INT4 ^ 2) AS grouped_power");

    // Assert
    let result = result.expect("unary minus and parentheses follow PostgreSQL power precedence");
    assert_eq!(
        result.rows(),
        &[vec![Value::Float64(4.0), Value::Float64(-4.0)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701]
    );
}
