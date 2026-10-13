//! PostgreSQL 18 temporal function syntax tracked by #815.

#[test]
fn should_parse_overlaps_period_operator() {
    // Arrange
    let sql = "SELECT (TIMESTAMP '2001-02-16 00:00:00', INTERVAL '1 day') OVERLAPS (TIMESTAMP '2001-02-16 12:00:00', INTERVAL '1 day')";

    // Act
    let parsed = cassie::sql::parse_statement(sql);

    // Assert
    assert!(parsed.is_ok(), "OVERLAPS period operator: {parsed:?}");
}
