#[test]
fn should_index_a_postgres_array_after_casting_it() {
    // Arrange
    let fixture = sql_fixture("issue-796-array-suffix-after-cast", &[]);

    // Act
    let result = fixture.execute("SELECT ('{10,20}'::INT[])[2] AS second_element");

    // Assert
    let result = result.expect("array subscript follows the cast expression");
    assert_eq!(result.rows(), &[vec![Value::Int64(20)]]);
    assert_eq!(result.columns()[0].type_oid, 23);
}
