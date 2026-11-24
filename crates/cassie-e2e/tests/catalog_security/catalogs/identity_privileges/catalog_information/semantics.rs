#[test]
fn should_report_current_user_with_postgresql_name_identity() {
    // Arrange
    let fixture = sql_fixture("issue-817-current-user-name-identity", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let user = fixture
        .execute(
            "SELECT current_user AS current_name, \
                    session_user AS session_name, current_role AS role_name",
        )
        .expect("read the active user name");

    // Assert
    assert_eq!(
        user.columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![("current_name", 19), ("session_name", 19), ("role_name", 19)]
    );
    assert_eq!(
        user.rows(),
        vec![vec![
            Value::String("tester".to_owned()),
            Value::String("tester".to_owned()),
            Value::String("tester".to_owned()),
        ]]
    );
}
