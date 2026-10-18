#[test]
fn should_truncate_utf8_quoted_identifiers_at_postgres_byte_limit() {
    // Arrange
    let fixture = sql_fixture("issue-796-utf8-identifier-truncation", &[]);
    let retained = format!("{}éx", "a".repeat(60));
    let oversized = format!("{}éxyz", "a".repeat(60));

    // Act
    let result = fixture.execute(&format!(
        "SELECT \"{retained}\" FROM (SELECT 7 AS \"{oversized}\") AS names"
    ));

    // Assert
    assert_eq!(
        result
            .expect("PostgreSQL truncates identifiers by UTF-8 byte length")
            .rows(),
        &[vec![Value::Int64(7)]]
    );
}
