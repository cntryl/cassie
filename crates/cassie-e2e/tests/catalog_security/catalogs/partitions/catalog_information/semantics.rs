#[test]
fn should_report_no_partitioned_relations_in_a_fresh_database() {
    // Arrange
    let fixture = sql_fixture("issue-817-partitioned-table-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) FROM pg_catalog.pg_partitioned_table")
        .expect("query the PostgreSQL partitioned table catalog");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}
