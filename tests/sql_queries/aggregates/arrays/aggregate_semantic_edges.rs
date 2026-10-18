#[test]
fn should_filter_distinct_array_aggregation_by_ordered_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-array-aggregate-distinct-order-filter",
        &[
            "CREATE TABLE issue_811_ordered_array_values (position INT, selected BOOLEAN, value INT)",
            "INSERT INTO issue_811_ordered_array_values VALUES (1,TRUE,2),(2,FALSE,99),(3,TRUE,NULL),(4,TRUE,1),(5,TRUE,2),(6,TRUE,NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT ARRAY_AGG(DISTINCT value ORDER BY value DESC) FILTER (WHERE selected)::TEXT FROM issue_811_ordered_array_values",
    );

    // Assert
    let result = result.expect("array_agg filters before DISTINCT and aggregate-local ordering");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("{NULL,2,1}".to_owned())]]
    );
    assert_eq!(result.columns()[0].type_oid, 25);
}
