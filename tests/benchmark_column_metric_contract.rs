#[test]
fn should_validate_direct_aggregate_metrics_without_requiring_row_scan_counters() {
    // Arrange
    let source = include_str!("../benches/tier3_system_query.rs");
    let start = source
        .find("fn bench_column_representative")
        .expect("column benchmark function");
    let end = source[start..]
        .find("fn bench_vector_exact_representative")
        .map(|offset| start + offset)
        .expect("next benchmark function");
    let column_benchmark = &source[start..end];

    // Act
    let validates_direct_scan = column_benchmark.contains(
        "assert_metric_increased(&before, &after, \"aggregate_acceleration\", \"scans\")",
    );
    let validates_selected_rows = column_benchmark.contains(
        "assert_metric_increased(&before, &after, \"column_batches\", \"selected_rows\")",
    );
    let requires_projected_scan = [
        "\"scans\"",
        "\"predicate_values\"",
        "\"materialized_values\"",
    ]
    .into_iter()
    .any(|metric| {
        column_benchmark.contains(&format!(
            "assert_metric_increased(&before, &after, \"column_batches\", {metric})"
        ))
    });

    // Assert
    assert!(validates_direct_scan);
    assert!(validates_selected_rows);
    assert!(!requires_projected_scan);
}
