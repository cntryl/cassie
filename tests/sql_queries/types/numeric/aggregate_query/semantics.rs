#[test]
fn should_apply_positive_numeric_range_offsets_to_window_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-positive-range-offsets",
        &[
            "CREATE TABLE issue_806_range_offsets (id INT, n INT)",
            "INSERT INTO issue_806_range_offsets VALUES (10,1),(20,2),(40,4),(60,6)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT n, FIRST_VALUE(id) OVER (ORDER BY n RANGE BETWEEN 2 PRECEDING AND 1 FOLLOWING), LAST_VALUE(id) OVER (ORDER BY n RANGE BETWEEN 2 PRECEDING AND 1 FOLLOWING) FROM issue_806_range_offsets ORDER BY n",
    );

    // Assert
    let result = result.expect("positive numeric RANGE offsets for window values");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(20)],
            vec![Value::Int64(2), Value::Int64(10), Value::Int64(20)],
            vec![Value::Int64(4), Value::Int64(20), Value::Int64(40)],
            vec![Value::Int64(6), Value::Int64(40), Value::Int64(60)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23]
    );
}
