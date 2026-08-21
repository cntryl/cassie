#[test]
fn should_bound_durable_benchmark_document_write_batches() {
    // Arrange
    let fixture = include_str!("../benches/support/workloads/context.rs");

    // Act
    let declares_bounded_batch =
        fixture.contains("const BENCH_DOCUMENT_WRITE_BATCH_ROWS: usize = 10_000;");
    let writes_bounded_batches = fixture.contains(".take(BENCH_DOCUMENT_WRITE_BATCH_ROWS)");

    // Assert
    assert!(declares_bounded_batch);
    assert!(writes_bounded_batches);
}
