//! PostgreSQL 18 mathematical operator grammar tracked by #813.

#[test]
fn should_parse_left_associative_numeric_power_operators() {
    // Arrange
    let sql = "SELECT 2 ^ 3 ^ 3";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "numeric power operator: {parsed:?}");
}

#[test]
fn should_parse_the_prefix_square_root_operator() {
    // Arrange
    let sql = "SELECT |/ 25.0";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "square-root operator: {parsed:?}");
}

#[test]
fn should_parse_the_prefix_cube_root_operator() {
    // Arrange
    let sql = "SELECT ||/ 64.0";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "cube-root operator: {parsed:?}");
}
