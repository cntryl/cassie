#[path = "../benches/support/workloads.rs"]
mod workloads;

#[test]
fn should_expose_vectorized_join_fixture_to_sql() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime");
    let context = runtime
        .block_on(workloads::vectorized_join_context(
            "join-fixture-sql-contract",
            4,
        ))
        .expect("join fixture");

    // Act
    let result = context.cassie.execute_sql(
        &context.session,
        "SELECT bench_join_users.name FROM bench_join_users JOIN bench_join_orders ON bench_join_users.user_key = bench_join_orders.order_user_key LIMIT 1",
        vec![],
    );

    // Assert
    assert!(
        result.is_ok(),
        "join fixture must be SQL-visible: {result:?}"
    );
}
