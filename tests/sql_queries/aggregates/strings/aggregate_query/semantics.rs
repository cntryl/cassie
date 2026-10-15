#[test]
fn should_deduplicate_string_aggregate_inputs_before_ordering() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-distinct-ordered-string-aggregate",
        &[
            "CREATE TABLE issue_804_distinct_ordered_values (value TEXT)",
            "INSERT INTO issue_804_distinct_ordered_values VALUES ('pear'),('apple'),('pear'),('banana')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT STRING_AGG(DISTINCT value, ',' ORDER BY value DESC) FROM issue_804_distinct_ordered_values",
    );

    // Assert
    assert_eq!(
        result
            .expect("distinct inputs ordered inside string_agg")
            .rows(),
        &[vec![Value::String("pear,banana,apple".to_owned())]]
    );
}

#[test]
fn should_filter_rows_before_distinct_ordered_string_aggregation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-filter-distinct-order-string-aggregate",
        &[
            "CREATE TABLE issue_804_filtered_distinct_order (value TEXT, selected BOOLEAN)",
            "INSERT INTO issue_804_filtered_distinct_order VALUES ('pear',TRUE),('apple',TRUE),('pear',FALSE),('banana',TRUE),('zebra',FALSE),('banana',FALSE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT STRING_AGG(DISTINCT value, ',' ORDER BY value DESC) FILTER (WHERE selected) FROM issue_804_filtered_distinct_order",
    );

    // Assert
    let result = result.expect("FILTER precedes DISTINCT and aggregate-local ordering");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("pear,banana,apple".to_owned())]]
    );
    assert_eq!(result.columns()[0].type_oid, 25);
}

#[test]
fn should_order_filtered_string_aggregation_by_a_separate_key() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-filtered-string-aggregate-separate-order-key",
        &[
            "CREATE TABLE issue_804_ordered_filtered_string_values (bucket TEXT, label TEXT, priority INT, selected BOOLEAN)",
            "INSERT INTO issue_804_ordered_filtered_string_values VALUES ('north','alpha',20,TRUE),('north','bravo',30,TRUE),('north','charlie',10,TRUE),('north','ignored',40,FALSE),('north','bravo',0,TRUE),('south','delta',5,TRUE),('south','echo',6,FALSE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, STRING_AGG(label, ',' ORDER BY priority DESC) FILTER (WHERE selected) FROM issue_804_ordered_filtered_string_values GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("filtered aggregate orders by a non-input key within each group");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("north".to_owned()),
                Value::String("bravo,alpha,charlie,bravo".to_owned()),
            ],
            vec![
                Value::String("south".to_owned()),
                Value::String("delta".to_owned()),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25]
    );
}
