#[test]
fn should_aggregate_jsonb_object_key_value_pairs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-jsonb-object-aggregate-distinct-keys",
        &[
            "CREATE TABLE issue_811_object_aggregate_values (key TEXT, value INT)",
            "INSERT INTO issue_811_object_aggregate_values VALUES ('a',1),('b',2)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT jsonb_object_agg(key, value)::TEXT FROM issue_811_object_aggregate_values",
    );

    // Assert
    assert_eq!(
        result
            .expect("JSONB object aggregation returns the input key-value pairs")
            .rows(),
        &[vec![Value::String("{\"a\": 1, \"b\": 2}".to_owned())]]
    );
}
