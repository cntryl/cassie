#[test]
fn should_return_postgres_input_error_details_without_raising_for_invalid_text() {
    // Arrange
    let fixture = sql_fixture("issue-796-input-error-info", &[]);

    // Act
    let result = fixture.execute("SELECT * FROM pg_input_error_info('bad', 'integer')");

    // Assert
    let result = result.expect("pg_input_error_info reports conversion failure as data");
    assert_eq!(result.rows().len(), 1);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["message", "detail", "hint", "sql_error_code"]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("invalid input syntax for type integer: \"bad\"".to_owned()),
            Value::Null,
            Value::Null,
            Value::String("22P02".to_owned()),
        ]]
    );
}
