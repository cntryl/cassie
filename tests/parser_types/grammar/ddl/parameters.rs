#[test]
fn should_bind_schema_qualified_postgres_type_names() {
    // Arrange
    let fixture = sql_fixture("issue-796-qualified-type-name", &[]);

    // Act
    let result = fixture.execute("SELECT '7'::pg_catalog.int4 AS value");

    // Assert
    assert_eq!(
        result.expect("schema-qualified PostgreSQL type").rows(),
        &[vec![Value::Int64(7)]]
    );
}
