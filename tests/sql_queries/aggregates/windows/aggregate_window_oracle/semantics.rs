#[test]
fn should_apply_filter_after_exclude_ties_for_window_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-filter-and-exclude-ties-window-aggregate",
        &[
            "CREATE TABLE issue_806_filtered_ties (score INT, id INT, amount INT, selected BOOLEAN)",
            "INSERT INTO issue_806_filtered_ties VALUES (1,1,10,TRUE),(2,2,20,TRUE),(2,3,30,FALSE),(3,4,40,TRUE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT score, id, amount, selected, \
         SUM(amount) FILTER (WHERE selected) OVER ( \
             ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE TIES \
         ), \
         COUNT(*) FILTER (WHERE selected) OVER ( \
             ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE TIES \
         ) \
         FROM issue_806_filtered_ties ORDER BY score, id",
    );

    // Assert
    let result = result.expect("FILTER and EXCLUDE TIES compose over the same window frame");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(1),
                Value::Int64(10),
                Value::Bool(true),
                Value::Int64(70),
                Value::Int64(3),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(2),
                Value::Int64(20),
                Value::Bool(true),
                Value::Int64(70),
                Value::Int64(3),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(3),
                Value::Int64(30),
                Value::Bool(false),
                Value::Int64(50),
                Value::Int64(2),
            ],
            vec![
                Value::Int64(3),
                Value::Int64(4),
                Value::Int64(40),
                Value::Bool(true),
                Value::Int64(70),
                Value::Int64(3),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23, 16, 20, 20]
    );
}
