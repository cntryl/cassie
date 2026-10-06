//! Bounded demand for column-index projections.
use crate::support_column_projection_fixture::fixture;
use cassie::midge::adapter::query_scan_control_test_guard;
use cassie::types::Value;

#[test]
fn should_stop_encoded_projection_at_limit_before_retaining_later_segments() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let (cassie, session, path, payload) = fixture();

    // Act
    let result = cassie.execute_sql(&session, "SELECT payload FROM records LIMIT 1", vec![]);

    // Assert
    let result = result.expect("LIMIT must not retain unused encoded segments");
    assert_eq!(result.rows, vec![vec![Value::String(payload)]]);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    drop((session, cassie));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_read_all_encoded_projection_rows_without_retaining_all_segment_scratch() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let (cassie, session, path, payload) = fixture();

    // Act
    let result = cassie.execute_sql(&session, "SELECT payload FROM records", vec![]);

    // Assert
    let result = result.expect("bounded output must not require retaining all encoded scratch");
    assert_eq!(result.rows, vec![vec![Value::String(payload)]; 8]);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    drop((session, cassie));
    let _ = std::fs::remove_dir_all(path);
}
