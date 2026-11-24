#[test]
fn should_apply_collection_aggregate_input_null_policies() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-aggregate-null-and-empty-results",
        &[
            "CREATE TABLE issue_811_aggregate_null_values (label TEXT, amount INT, payload JSONB)",
            "INSERT INTO issue_811_aggregate_null_values VALUES (NULL,NULL,NULL)",
        ],
    );

    // Act
    let all_null = fixture.execute(
        "SELECT STRING_AGG(label, ','), ARRAY_AGG(amount)::TEXT, JSONB_AGG(payload)::TEXT, JSONB_OBJECT_AGG('k', amount)::TEXT FROM issue_811_aggregate_null_values",
    );
    let empty = fixture.execute(
        "SELECT STRING_AGG(label, ','), ARRAY_AGG(amount), JSONB_AGG(payload), JSONB_OBJECT_AGG('k', amount) FROM issue_811_aggregate_null_values WHERE FALSE",
    );

    // Assert
    assert_eq!(
        all_null
            .expect("non-strict aggregates retain their PostgreSQL NULL policies")
            .rows(),
        &[vec![
            Value::Null,
            Value::String("{NULL}".to_owned()),
            Value::String("[null]".to_owned()),
            Value::String("{\"k\": null}".to_owned()),
        ]]
    );
    let empty = empty.expect("collection aggregates return NULL for no input rows");
    assert_eq!(
        empty.rows(),
        &[vec![Value::Null, Value::Null, Value::Null, Value::Null]]
    );
    assert_eq!(
        empty
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 1_007, 3_802, 3_802]
    );
}
