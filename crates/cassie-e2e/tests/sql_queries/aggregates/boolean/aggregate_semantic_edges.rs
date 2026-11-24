#[test]
fn should_return_null_for_boolean_aggregate_without_nonnull_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-805-empty-null-boolean-aggregates",
        &[
            "CREATE TABLE issue_805_boolean_null_inputs (flag BOOLEAN)",
            "INSERT INTO issue_805_boolean_null_inputs VALUES (NULL), (NULL)",
        ],
    );

    // Act
    let all_null = fixture.execute(
        "SELECT BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) FROM issue_805_boolean_null_inputs",
    );
    let empty = fixture.execute(
        "SELECT BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) \
         FROM issue_805_boolean_null_inputs WHERE FALSE",
    );

    // Assert
    for result in [all_null, empty] {
        let result = result.expect("boolean aggregates over no non-NULL inputs");
        assert_eq!(
            result.rows(),
            &[vec![Value::Null, Value::Null, Value::Null]]
        );
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![16, 16, 16]
        );
    }
}

#[test]
fn should_accept_distinct_boolean_aggregate_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-805-distinct-boolean-aggregates",
        &[
            "CREATE TABLE issue_805_boolean_distinct (flag BOOLEAN)",
            "INSERT INTO issue_805_boolean_distinct VALUES (TRUE),(TRUE),(FALSE),(FALSE),(NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT BOOL_AND(DISTINCT flag), BOOL_OR(DISTINCT flag) FROM issue_805_boolean_distinct",
    );

    // Assert
    let result = result.expect("DISTINCT Boolean aggregates over duplicate and NULL inputs");
    assert_eq!(
        result.rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
}
