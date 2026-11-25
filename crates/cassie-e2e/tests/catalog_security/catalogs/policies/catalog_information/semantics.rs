#[test]
fn should_report_no_policies_for_a_plain_table() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-policy-empty-table",
        &["CREATE TABLE issue_817_plain_policy_table (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT count(*) \
             FROM pg_catalog.pg_policy AS policy \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = policy.polrelid \
             WHERE relation.relname = 'issue_817_plain_policy_table'",
        )
        .expect("query row security policies for the plain table");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}
