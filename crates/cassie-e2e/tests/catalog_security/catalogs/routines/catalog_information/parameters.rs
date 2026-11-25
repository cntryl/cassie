#[test]
fn should_report_no_user_parameter_privilege_rows_in_a_fresh_database() {
    // Arrange
    let fixture = sql_fixture("issue-817-parameter-acl-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) FROM pg_catalog.pg_parameter_acl")
        .expect("query PostgreSQL parameter access control metadata");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}
