#[test]
fn should_return_postgres_bitwise_aggregate_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-bitwise-aggregates",
        &[
            "CREATE TABLE issue_806_bitwise_values (n INT)",
            "INSERT INTO issue_806_bitwise_values VALUES (1),(2),(3),(4)",
        ],
    );
    let cases = [
        (
            "SELECT bit_and(n) FROM issue_806_bitwise_values",
            Value::Int64(0),
        ),
        (
            "SELECT bit_or(n) FROM issue_806_bitwise_values",
            Value::Int64(7),
        ),
        (
            "SELECT bit_xor(n) FROM issue_806_bitwise_values",
            Value::Int64(4),
        ),
    ];

    // Act
    let failures = cases
        .iter()
        .filter_map(|(sql, expected)| match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![expected.clone()]] => None,
            Ok(result) => Some(format!(
                "{sql}: expected {expected:?}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "bitwise aggregates differ:\n{}",
        failures.join("\n")
    );
}
