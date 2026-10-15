#[test]
fn should_bind_double_precision_type_names_in_casts() {
    // Arrange
    let fixture = sql_fixture("issue-796-double-precision", &[]);

    // Act
    let result = fixture.execute("SELECT CAST('1.5' AS DOUBLE PRECISION) AS value");

    // Assert
    assert_eq!(
        result.expect("DOUBLE PRECISION cast").rows(),
        &[vec![Value::Float64(1.5)]]
    );
}

#[test]
fn should_bind_postgres_chained_type_casts() {
    // Arrange
    let fixture = sql_fixture("issue-796-chained-type-casts", &[]);

    // Act
    let result = fixture.execute("SELECT '7'::TEXT::INT4 AS chained");

    // Assert
    let result = result.expect("chained casts bind through the preceding target type");
    assert_eq!(result.rows(), &[vec![Value::Int64(7)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}
