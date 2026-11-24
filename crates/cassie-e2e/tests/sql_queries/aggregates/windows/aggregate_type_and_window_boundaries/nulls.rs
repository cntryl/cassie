#[test]
fn should_keep_empty_frames_visible_at_window_partition_boundaries() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-frame-empty-boundaries",
        &[
            "CREATE TABLE issue_806_window_boundary_values (bucket TEXT, id INT, amount INT)",
            "INSERT INTO issue_806_window_boundary_values VALUES ('a',1,10),('a',2,20),('a',3,30),('b',1,5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, id, amount, SUM(amount) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING), COUNT(*) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING), SUM(amount) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING), COUNT(*) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 1 FOLLOWING) FROM issue_806_window_boundary_values ORDER BY bucket,id",
    );

    // Assert
    let result = result.expect("window frames preserve empty boundary results");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(1),
                Value::Int64(10),
                Value::Null,
                Value::Int64(0),
                Value::Int64(20),
                Value::Int64(1),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(20),
                Value::Int64(10),
                Value::Int64(1),
                Value::Int64(30),
                Value::Int64(1),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(3),
                Value::Int64(30),
                Value::Int64(20),
                Value::Int64(1),
                Value::Null,
                Value::Int64(0),
            ],
            vec![
                Value::String("b".to_owned()),
                Value::Int64(1),
                Value::Int64(5),
                Value::Null,
                Value::Int64(0),
                Value::Null,
                Value::Int64(0),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 23, 20, 20, 20, 20]
    );
}
