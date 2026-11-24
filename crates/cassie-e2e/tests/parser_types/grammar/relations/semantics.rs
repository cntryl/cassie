#[test]
fn should_concatenate_text_with_non_text_scalar_values() {
    // Arrange
    let fixture = sql_fixture("issue-796-polymorphic-text-concatenation", &[]);

    // Act
    let result = fixture.execute("SELECT 'Value: ' || 42 AS value");

    // Assert
    assert_eq!(
        result.expect("text concatenation with integer").rows(),
        &[vec![Value::String("Value: 42".to_owned())]]
    );
}

#[test]
fn should_return_postgres_float_special_values() {
    // Arrange
    let fixture = sql_fixture("issue-796-float-special-values", &[]);

    // Act
    let cases: [(&str, ValuePredicate); 6] = [
        (
            "SELECT CAST('NaN' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_nan()),
        ),
        (
            "SELECT CAST('Infinity' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_positive()),
        ),
        (
            "SELECT CAST('-Infinity' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_negative()),
        ),
        (
            "SELECT CAST('inf' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_positive()),
        ),
        (
            "SELECT CAST('-inf' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_infinite() && number.is_sign_negative()),
        ),
        (
            "SELECT CAST('nan' AS FLOAT) AS value",
            |value| matches!(value, Value::Float64(number) if number.is_nan()),
        ),
    ];
    let failures = cases
        .iter()
        .filter_map(|(sql, is_valid)| match fixture.execute(sql) {
            Ok(result)
                if result
                    .rows()
                    .first()
                    .and_then(|row| row.first())
                    .is_some_and(is_valid) =>
            {
                None
            }
            Ok(result) => Some(format!("{sql}: unexpected result {:?}", result.rows())),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "floating-point special values failed:\n{}",
        failures.join("\n")
    );
}
