#[test]
fn should_skip_nulls_in_bitwise_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-bitwise-empty-and-null-inputs",
        &[
            "CREATE TABLE issue_806_bitwise_null_values (n INT)",
            "INSERT INTO issue_806_bitwise_null_values VALUES (NULL),(NULL)",
        ],
    );

    // Act
    let all_null = fixture.execute(
        "SELECT bit_and(n), bit_or(n), bit_xor(n), COUNT(n) FROM issue_806_bitwise_null_values",
    );
    let empty = fixture.execute(
        "SELECT bit_and(n), bit_or(n), bit_xor(n), COUNT(n) FROM issue_806_bitwise_null_values WHERE FALSE",
    );

    // Assert
    let expected = &[vec![Value::Null, Value::Null, Value::Null, Value::Int64(0)]];
    for result in [all_null, empty] {
        let result = result.expect("bitwise aggregates ignore NULL and return NULL without values");
        assert_eq!(result.rows(), expected);
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![23, 23, 23, 20]
        );
    }
}
