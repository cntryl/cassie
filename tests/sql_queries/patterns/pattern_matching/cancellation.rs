#[test]
fn should_cancel_recursive_regex_work_with_cleanup() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.cte_recursion_depth = 1_000_000;
    config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
    let fixture = std::sync::Arc::new(sql_fixture_with_config(
        "issue-799-regex-active-cancellation",
        &[],
        config,
    ));
    let memory_before = current_accounted_memory_bytes(&fixture);
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let query_fixture = std::sync::Arc::clone(&fixture);
    let query = std::thread::spawn(move || {
        query_fixture.cassie.execute_sql_with_cancellation(
            &query_fixture.session,
            "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000 AND n::TEXT ~ '^[0-9]+$') SELECT MAX(n) FROM seq",
            vec![],
            &query_cancellation,
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(25));

    // Act
    cancellation.cancel();
    let error = query
        .join()
        .expect("regex query thread")
        .expect_err("active regex query should be cancelled");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(current_accounted_memory_bytes(&fixture), memory_before);
    assert_eq!(
        fixture
            .execute("SELECT 1::BIGINT")
            .expect("query after regex cancellation")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}
