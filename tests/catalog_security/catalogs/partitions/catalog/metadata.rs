#[test]
fn should_report_partitioned_table_catalog_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-partitioned-table-row", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE TABLE issue_817_partitioned_catalog_parent (id INTEGER, payload TEXT) \
             PARTITION BY RANGE (id)",
        )
        .expect("create a range-partitioned parent");
    fixture
        .execute(
            "CREATE TABLE issue_817_partitioned_catalog_child \
             PARTITION OF issue_817_partitioned_catalog_parent \
             FOR VALUES FROM (0) TO (10)",
        )
        .expect("create a valid range partition");
    let parent_kind = fixture
        .execute(
            "SELECT relkind::text FROM pg_catalog.pg_class \
             WHERE oid = 'issue_817_partitioned_catalog_parent'::regclass",
        )
        .expect("confirm the partitioned parent exists in pg_class");
    assert_eq!(parent_kind.rows(), &[vec![Value::String("p".to_owned())]]);

    // Act
    let rows = fixture
        .execute(
            "SELECT partstrat::text, partnatts, partdefid, partattrs::text \
             FROM pg_catalog.pg_partitioned_table \
             WHERE partrelid = 'issue_817_partitioned_catalog_parent'::regclass",
        )
        .expect("read the partition strategy and key from pg_partitioned_table")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("r".to_owned()),
            Value::Int64(1),
            Value::Int64(0),
            Value::String("1".to_owned()),
        ]]
    );
}
