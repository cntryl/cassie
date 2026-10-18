#[test]
fn should_assign_ntile_buckets_by_partition_with_null_scores() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-ntile-partition-peer-identity",
        &[
            "CREATE TABLE issue_806_ntile_rows (group_id INT, score INT, id INT PRIMARY KEY)",
            "INSERT INTO issue_806_ntile_rows VALUES (1,10,1),(1,20,2),(1,20,3),(1,NULL,4),(2,5,5),(2,5,6)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT group_id, id, \
         NTILE(3) OVER (PARTITION BY group_id ORDER BY score NULLS LAST, id) \
         FROM issue_806_ntile_rows ORDER BY group_id, id",
    );

    // Assert
    let result = result.expect("NTILE executes over repeated scores and NULL rows");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(2), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(3), Value::Int64(2)],
            vec![Value::Int64(1), Value::Int64(4), Value::Int64(3)],
            vec![Value::Int64(2), Value::Int64(5), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(6), Value::Int64(2)],
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
