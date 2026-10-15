#[test]
fn should_accept_postgres_postfix_null_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-796-postfix-null-predicates", &[]);

    // Act
    let result = fixture.execute("SELECT NULL ISNULL, 1 NOTNULL");

    // Assert
    assert_eq!(
        result.expect("postfix null predicates").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}
