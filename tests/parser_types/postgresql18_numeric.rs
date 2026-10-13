//! PostgreSQL 18 NUMERIC and DECIMAL cast grammar tracked by #812.

#[test]
fn should_parse_numeric_precision_scale_modifiers() {
    // Arrange
    let sql = "SELECT CAST('12.345' AS NUMERIC(5, 2))";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "NUMERIC(precision, scale): {parsed:?}");
}

#[test]
fn should_parse_decimal_alias_with_negative_scale() {
    // Arrange
    let sql = "SELECT CAST('12500' AS DECIMAL(2, -3))";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "DECIMAL negative scale: {parsed:?}");
}
