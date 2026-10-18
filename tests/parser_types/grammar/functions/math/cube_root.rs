#[test]
fn should_evaluate_the_prefix_cube_root_operator_as_float8() {
    // Arrange
    let fixture = sql_fixture("issue-813-prefix-cube-root", &[]);

    // Act
    let result = fixture.execute("SELECT ||/ 64.0 AS value");

    // Assert
    let result = result.expect("PostgreSQL prefix cube-root operator");
    assert_eq!(result.rows(), &[vec![Value::Float64(4.0)]]);
    assert_eq!(result.columns()[0].type_oid, 701);
}
