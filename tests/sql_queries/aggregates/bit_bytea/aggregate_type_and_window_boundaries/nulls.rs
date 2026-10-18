#[test]
fn should_preserve_bitwise_aggregate_type_identity_across_empty_null_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-bitwise-aggregate-input-type-identity",
        &[
            "CREATE TABLE issue_806_bitwise_type_values (small_value SMALLINT, int_value INTEGER, big_value BIGINT)",
            "INSERT INTO issue_806_bitwise_type_values VALUES (-2,-2,-2),(7,7,7),(NULL,NULL,NULL)",
        ],
    );
    let values_query = "SELECT bit_and(small_value), bit_or(small_value), bit_xor(small_value), bit_and(int_value), bit_or(int_value), bit_xor(int_value), bit_and(big_value), bit_or(big_value), bit_xor(big_value) FROM issue_806_bitwise_type_values";
    let all_null_query = format!("{values_query} WHERE small_value IS NULL");
    let empty_query = format!("{values_query} WHERE FALSE");

    // Act
    let values = fixture.execute(values_query);
    let all_null = fixture.execute(&all_null_query);
    let empty = fixture.execute(&empty_query);

    // Assert
    let expected_oids = vec![21, 21, 21, 23, 23, 23, 20, 20, 20];
    let values = values.expect("bitwise aggregates retain each integer input width");
    assert_eq!(
        values.rows(),
        &[vec![
            Value::Int64(6),
            Value::Int64(-1),
            Value::Int64(-7),
            Value::Int64(6),
            Value::Int64(-1),
            Value::Int64(-7),
            Value::Int64(6),
            Value::Int64(-1),
            Value::Int64(-7),
        ]]
    );
    assert_eq!(
        values
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );

    let expected_nulls = &[vec![Value::Null; 9]];
    for result in [all_null, empty] {
        let result = result.expect("bitwise aggregates return NULL without non-NULL inputs");
        assert_eq!(result.rows(), expected_nulls);
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            expected_oids
        );
    }
}
