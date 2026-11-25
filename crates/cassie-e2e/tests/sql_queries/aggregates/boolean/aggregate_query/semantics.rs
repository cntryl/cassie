#[test]
fn should_apply_filter_before_boolean_aggregate_transitions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-805-filtered-boolean-aggregates",
        &[
            "CREATE TABLE issue_805_bool_values (bucket TEXT, flag BOOLEAN, selected BOOLEAN)",
            "INSERT INTO issue_805_bool_values VALUES ('mixed',TRUE,TRUE),('mixed',FALSE,TRUE),('mixed',NULL,FALSE),('none',TRUE,FALSE),('nulls',NULL,TRUE),('true',TRUE,TRUE),('true',TRUE,TRUE),('false',FALSE,TRUE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, BOOL_AND(flag) FILTER (WHERE selected), BOOL_OR(flag) FILTER (WHERE selected), COUNT(flag) FILTER (WHERE selected) FROM issue_805_bool_values GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("filtered boolean aggregates by group");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("false".to_owned()),
                Value::Bool(false),
                Value::Bool(false),
                Value::Int64(1),
            ],
            vec![
                Value::String("mixed".to_owned()),
                Value::Bool(false),
                Value::Bool(true),
                Value::Int64(2),
            ],
            vec![
                Value::String("none".to_owned()),
                Value::Null,
                Value::Null,
                Value::Int64(0),
            ],
            vec![
                Value::String("nulls".to_owned()),
                Value::Null,
                Value::Null,
                Value::Int64(0),
            ],
            vec![
                Value::String("true".to_owned()),
                Value::Bool(true),
                Value::Bool(true),
                Value::Int64(2),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 16, 16, 20]
    );
}
