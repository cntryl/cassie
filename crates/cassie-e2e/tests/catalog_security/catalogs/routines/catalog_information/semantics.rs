#[test]
fn should_report_builtin_function_result_identity_in_pg_proc() {
    // Arrange
    let fixture = sql_fixture("issue-817-procedure-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT proname, prorettype FROM pg_catalog.pg_proc WHERE proname = 'now'")
        .expect("query the built-in procedure catalog");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("now".to_owned()), Value::Int64(1_184)]]
    );
}
