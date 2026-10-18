#[test]
fn should_require_finite_regex_execution_before_active_cancellation() {
    // Arrange
    // #799 must implement regex execution before #954 can synchronize active cancellation.
    let fixture = sql_fixture("issue-799-regex-cancellation-prerequisite", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 3 AND n::TEXT ~ '^[0-9]+$') SELECT MAX(n) FROM seq",
    );

    // Assert
    assert_eq!(result.expect("regex execution prerequisite").rows(), &[vec![Value::Int64(3)]]);
}
