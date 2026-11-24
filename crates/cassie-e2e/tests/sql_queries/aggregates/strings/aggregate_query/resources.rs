#[test]
fn should_accept_a_bytea_delimiter_in_string_agg() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-bytea-string-aggregate",
        &[
            "CREATE TABLE issue_811_bytea_string_values (hex_value TEXT)",
            "INSERT INTO issue_811_bytea_string_values VALUES ('6161')",
            "INSERT INTO issue_811_bytea_string_values VALUES ('6262')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT string_agg(decode(hex_value,'hex'), decode('2c','hex')) = decode('61612c6262','hex') FROM issue_811_bytea_string_values",
    );

    // Assert
    let result = result.expect("bytea string_agg accepts a bytea delimiter");
    assert_eq!(result.rows(), &[vec![Value::Bool(true)]]);
}
