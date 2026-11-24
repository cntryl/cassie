#[test]
fn should_concatenate_a_numeric_expression_before_text() {
    // Arrange
    let fixture = sql_fixture("issue-796-non-text-before-text-concatenation", &[]);

    // Act
    let result = fixture.execute("SELECT (1 + 2) || 'b' AS value");

    // Assert
    let result = result.expect("a non-array scalar may precede a text operand");
    assert_eq!(result.rows(), &[vec![Value::String("3b".to_owned())]]);
    assert_eq!(result.columns()[0].type_oid, 25);
}
