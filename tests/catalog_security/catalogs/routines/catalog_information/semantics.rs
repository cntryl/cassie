#[test]
fn should_list_visible_builtin_routines_in_information_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-information-schema-routines", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT routine_schema, routine_name, routine_type \
             FROM information_schema.routines \
             WHERE routine_schema = 'pg_catalog' AND routine_name = 'now'",
        )
        .expect("query built-in routine metadata");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("pg_catalog".to_owned()),
            Value::String("now".to_owned()),
            Value::String("FUNCTION".to_owned()),
        ]]
    );
}

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
