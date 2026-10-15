#[test]
fn should_resolve_schema_qualified_binary_operators() {
    // Arrange
    let label = "issue-796-qualified-operators";
    let fixture = sql_fixture(label, &[]);

    // Act
    let result = fixture.execute("SELECT 1 OPERATOR(pg_catalog.+) 2 AS value");

    // Assert
    assert_eq!(
        result.expect("qualified binary operator").rows(),
        &[vec![Value::Int64(3)]]
    );
}
