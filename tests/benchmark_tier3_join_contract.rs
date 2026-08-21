#[test]
fn should_bound_tier3_analytical_queries_beyond_the_product_default_deadline() {
    // Arrange
    let fixture = include_str!("../benches/support/workloads/tier3_query_fixture.rs");

    // Act
    let uses_analytical_timeout = fixture
        .contains("config.limits.query_timeout_ms = LARGE_ANALYTICAL_BENCHMARK_QUERY_TIMEOUT_MS;");

    // Assert
    assert!(uses_analytical_timeout);
}

#[test]
fn should_index_the_bounded_tier3_join_fixture() {
    // Arrange
    let fixture = include_str!("../benches/support/workloads/tier3_query_fixture.rs");

    // Act
    let creates_join_index =
        fixture.contains("CREATE INDEX bench_join_users_key_idx ON bench_join_users (user_key)");

    // Assert
    assert!(creates_join_index);
}
