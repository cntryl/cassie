#[test]
fn should_apply_interval_range_offsets_to_timestamp_window_frames() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-timestamp-range-window-interval",
        &["CREATE TABLE issue_806_timestamp_window (instant TIMESTAMP, id INT, amount INT)"],
    );
    fixture
        .execute("INSERT INTO issue_806_timestamp_window VALUES ('2024-03-09 11:30:00', 1, 7), ('2024-03-09 12:00:00', 2, 10), ('2024-03-10 12:00:00', 3, 20), ('2024-03-11 12:00:00', 4, 30)")
        .unwrap_or_else(|error| panic!("timestamp table fixture insert failed: {error:?}"));

    // Act
    let result = fixture.execute(
        "SELECT id, FIRST_VALUE(id) OVER (ORDER BY instant RANGE BETWEEN INTERVAL '1 day' PRECEDING AND CURRENT ROW) FROM issue_806_timestamp_window ORDER BY instant",
    );

    // Assert
    let result = result.expect("timestamp RANGE frames accept interval offsets");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(1)],
            vec![Value::Int64(3), Value::Int64(2)],
            vec![Value::Int64(4), Value::Int64(3)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23]
    );
}
