#[test]
fn should_apply_exclusion_modes_to_grouped_window_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-grouped-window-aggregate-exclusion",
        &[
            "CREATE TABLE issue_806_grouped_window_values (score INT, amount INT)",
            "INSERT INTO issue_806_grouped_window_values VALUES (1,10),(2,20),(2,30),(3,40)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT score, amount, SUM(amount) OVER (ORDER BY score GROUPS BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE TIES), SUM(amount) OVER (ORDER BY score GROUPS BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE GROUP), SUM(amount) OVER (ORDER BY score GROUPS BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE CURRENT ROW) FROM issue_806_grouped_window_values ORDER BY score, amount",
    );

    // Assert
    assert_eq!(
        result
            .expect("window aggregates apply each exclusion mode to peer rows")
            .rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::Int64(10),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::Int64(2),
                Value::Int64(20),
                Value::Int64(20),
                Value::Null,
                Value::Int64(30),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(30),
                Value::Int64(30),
                Value::Null,
                Value::Int64(20),
            ],
            vec![
                Value::Int64(3),
                Value::Int64(40),
                Value::Int64(40),
                Value::Null,
                Value::Null,
            ],
        ]
    );
}

#[test]
fn should_apply_each_exclusion_mode_to_rows_window_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-rows-frame-aggregate-exclusions",
        &[
            "CREATE TABLE issue_806_rows_exclusion_values (score INT, amount INT)",
            "INSERT INTO issue_806_rows_exclusion_values VALUES (1,10),(2,20),(2,30),(3,40)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT score, amount, \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE NO OTHERS), \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE CURRENT ROW), \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE GROUP), \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE TIES) \
         FROM issue_806_rows_exclusion_values ORDER BY score, amount",
    );

    // Assert
    assert_eq!(
        result
            .expect("ROWS frames apply each exclusion mode to duplicate peers")
            .rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::Int64(100),
                Value::Int64(90),
                Value::Int64(90),
                Value::Int64(100),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(20),
                Value::Int64(100),
                Value::Int64(80),
                Value::Int64(50),
                Value::Int64(70),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(30),
                Value::Int64(100),
                Value::Int64(70),
                Value::Int64(50),
                Value::Int64(80),
            ],
            vec![
                Value::Int64(3),
                Value::Int64(40),
                Value::Int64(100),
                Value::Int64(60),
                Value::Int64(60),
                Value::Int64(100),
            ],
        ]
    );
}
