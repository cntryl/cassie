#[test]
fn should_apply_explicit_all_to_nullable_count_sum() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-all-aggregate-arguments",
        &[
            "CREATE TABLE issue_804_all_aggregate_values (bucket TEXT, amount INT)",
            "INSERT INTO issue_804_all_aggregate_values VALUES ('a',2),('a',2),('a',NULL),('b',NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, COUNT(ALL amount), SUM(ALL amount) FROM issue_804_all_aggregate_values GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("explicit ALL aggregate arguments");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(4),
            ],
            vec![Value::String("b".to_owned()), Value::Int64(0), Value::Null,],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 20, 20]
    );
}

#[test]
fn should_return_any_non_null_input_from_any_value_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1063-any-value-aggregate",
        &[
            "CREATE TABLE issue_1063_any_values (bucket INT, item INT)",
            "INSERT INTO issue_1063_any_values VALUES (1,10),(1,20),(2,NULL)",
            "CREATE TABLE issue_1063_any_value_empty (item INT)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, pg_catalog.any_value(item), COUNT(item) FROM issue_1063_any_values GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("ANY_VALUE over grouped inputs");
    assert_eq!(result.rows().len(), 2);
    let first = &result.rows()[0];
    assert_eq!(first[0], Value::Int64(1));
    assert!(
        matches!(first[1], Value::Int64(10 | 20)),
        "ANY_VALUE may select any non-NULL input, got {:?}",
        first[1]
    );
    assert_eq!(first[2], Value::Int64(2));
    assert_eq!(
        result.rows()[1],
        vec![Value::Int64(2), Value::Null, Value::Int64(0)]
    );

    let filtered = fixture
        .execute(
            "SELECT bucket, pg_catalog.any_value(item) FILTER (WHERE item > 10) FROM issue_1063_any_values GROUP BY bucket ORDER BY bucket",
        )
        .expect("filtered ANY_VALUE over grouped input");
    assert_eq!(filtered.columns()[1].type_oid, 23);
    assert_eq!(
        filtered.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(20)],
            vec![Value::Int64(2), Value::Null],
        ]
    );

    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );

    let empty = fixture
        .execute("SELECT ANY_VALUE(item) FROM issue_1063_any_value_empty")
        .expect("ANY_VALUE over an empty input");
    assert_eq!(empty.rows(), &[vec![Value::Null]]);
    assert_eq!(empty.columns()[0].type_oid, 23);
}
