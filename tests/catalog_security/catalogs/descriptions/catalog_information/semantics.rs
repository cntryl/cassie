#[test]
fn should_expose_postgresql_object_descriptions() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-description", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) > 0 FROM pg_catalog.pg_description")
        .expect("query PostgreSQL object descriptions");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Bool(true)]]);
}
