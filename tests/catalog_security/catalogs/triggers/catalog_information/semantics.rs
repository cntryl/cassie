#[test]
fn should_report_no_user_triggers_for_a_plain_table() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-trigger-empty-table",
        &["CREATE TABLE issue_817_plain_trigger_table (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT count(*) \
             FROM pg_catalog.pg_trigger AS trigger \
             JOIN pg_catalog.pg_class AS relation ON relation.oid = trigger.tgrelid \
             WHERE relation.relname = 'issue_817_plain_trigger_table'",
        )
        .expect("query triggers for the plain table");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_no_event_triggers_in_a_fresh_database() {
    // Arrange
    let fixture = sql_fixture("issue-817-event-trigger-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) FROM pg_catalog.pg_event_trigger")
        .expect("query the PostgreSQL event trigger catalog");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}
