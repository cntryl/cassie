#[test]
fn should_report_execute_privilege_for_a_builtin_function() {
    // Arrange
    let fixture = sql_fixture("issue-817-function-privilege", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.has_function_privilege( \
                'pg_catalog.abs(integer)', 'EXECUTE')",
        )
        .expect("check built-in function privilege");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
