#[test]
fn should_bind_text_parameters_in_ilike_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike-parameter", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT 'Alice' ILIKE $1 AS matched",
        vec![Value::String("a%".to_owned())],
    );

    // Assert
    assert_eq!(
        result.expect("bound ILIKE pattern").rows(),
        &[vec![Value::Bool(true)]]
    );
}
