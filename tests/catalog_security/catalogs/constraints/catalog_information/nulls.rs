#[test]
fn should_return_null_for_missing_constraint_definition() {
    // Arrange
    let fixture = sql_fixture("issue-817-null-constraint-definition", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_get_constraintdef(0)")
        .expect("inspect an absent constraint definition");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Null]]);
}
