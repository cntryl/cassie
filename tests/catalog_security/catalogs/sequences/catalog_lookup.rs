#[test]
fn should_report_usage_privilege_for_a_created_sequence() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-sequence-privilege",
        &["CREATE SEQUENCE issue_817_privilege_sequence"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.has_sequence_privilege( \
                'issue_817_privilege_sequence', 'USAGE')",
        )
        .expect("check the creating session's sequence privilege");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
