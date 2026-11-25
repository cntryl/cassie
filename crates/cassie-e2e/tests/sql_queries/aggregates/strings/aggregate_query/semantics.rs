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

#[test]
fn should_include_string_aggregate_delimiter_in_distinct_identity() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-distinct-string-aggregate-arguments",
        &[
            "CREATE TABLE issue_804_distinct_string_aggregate_arguments (value TEXT, separator TEXT)",
            "INSERT INTO issue_804_distinct_string_aggregate_arguments VALUES ('pear',','),('pear',';'),('pear',','),('apple',',')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT STRING_AGG(DISTINCT value, separator ORDER BY separator, value) FROM issue_804_distinct_string_aggregate_arguments",
    );

    // Assert
    assert_eq!(
        result
            .expect("DISTINCT string aggregate inputs include the delimiter")
            .rows(),
        &[vec![Value::String("apple,pear;pear".to_owned())]]
    );
}

#[test]
fn should_use_row_delimiters_while_skipping_null_string_aggregate_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-string-aggregate-null-inputs",
        &[
            "CREATE TABLE issue_804_string_aggregate_null_inputs (position INT, value TEXT, separator TEXT)",
            "INSERT INTO issue_804_string_aggregate_null_inputs VALUES (1,'first',','),(2,'second',NULL),(3,NULL,';'),(4,'third',',')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT STRING_AGG(value, separator ORDER BY position) FROM issue_804_string_aggregate_null_inputs",
    );

    // Assert
    assert_eq!(
        result
            .expect("NULL values are skipped and NULL delimiters append no separator")
            .rows(),
        &[vec![Value::String("firstsecond,third".to_owned())]]
    );
}

#[test]
fn should_accept_a_bound_text_delimiter_for_qualified_string_agg() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-bound-string-aggregate-delimiter",
        &[
            "CREATE TABLE issue_804_bound_string_aggregate (position INT, value TEXT)",
            "INSERT INTO issue_804_bound_string_aggregate VALUES (1,'first'),(2,'second')",
        ],
    );

    // Act
    let parameterized = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT pg_catalog.STRING_AGG(value, $1 ORDER BY position) FROM issue_804_bound_string_aggregate",
        vec![Value::String(".".to_owned())],
    );

    // Assert
    let parameterized = parameterized.expect("a bound TEXT delimiter is accepted");
    assert_eq!(
        parameterized.rows(),
        &[vec![Value::String("first.second".to_owned())]]
    );
    assert_eq!(parameterized.columns()[0].type_oid, 25);
}

#[test]
fn should_return_null_for_an_empty_string_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-empty-string-aggregate",
        &[
            "CREATE TABLE issue_804_empty_string_aggregate (value TEXT)",
            "INSERT INTO issue_804_empty_string_aggregate VALUES ('first'),('second')",
        ],
    );

    // Act
    let empty = fixture.execute(
        "SELECT STRING_AGG(value, ',') FILTER (WHERE FALSE) FROM issue_804_empty_string_aggregate",
    );

    // Assert
    assert_eq!(
        empty.expect("empty string aggregate returns SQL NULL").rows(),
        &[vec![Value::Null]]
    );
}
