use serde_json::json;

use super::{
    projected_payload_size_within, RollupSourceChangeReport, MAX_ROLLUP_CHANGE_REPORT_BYTES,
};

#[test]
fn should_drop_partial_rollup_change_report_after_size_limit() {
    // Arrange
    let fields = vec!["device".to_owned(), "event_at".to_owned()];
    let row = json!({ "device": "a", "event_at": "2026-01-01T00:00:00Z" });
    let oversized = json!({
        "device": "x".repeat(MAX_ROLLUP_CHANGE_REPORT_BYTES),
        "event_at": "2026-01-01T00:00:00Z"
    });
    let mut report = RollupSourceChangeReport::default();

    // Act
    report.record(None, Some(&row), &fields);
    report.record(None, Some(&oversized), &fields);

    // Assert
    assert!(report.exceeded_limit);
    assert!(report.rows.is_empty());
}

#[test]
fn should_reject_oversized_projected_scalars_during_size_preflight() {
    // Arrange
    let fields = vec!["device".to_owned()];
    let payload = json!({ "device": "x".repeat(MAX_ROLLUP_CHANGE_REPORT_BYTES + 1) });

    // Act
    let admitted =
        projected_payload_size_within(Some(&payload), &fields, MAX_ROLLUP_CHANGE_REPORT_BYTES);

    // Assert
    assert_eq!(admitted, None);
}

#[test]
fn should_accept_reports_at_the_64_kibibyte_boundary_and_reject_the_next_byte() {
    // Arrange
    let baseline = report_with_partition_bytes(0);
    let exact_partition_bytes = MAX_ROLLUP_CHANGE_REPORT_BYTES - baseline.retained_bytes;

    // Act
    let below = report_with_partition_bytes(exact_partition_bytes - 1);
    let exact = report_with_partition_bytes(exact_partition_bytes);
    let above = report_with_partition_bytes(exact_partition_bytes + 1);

    // Assert
    assert!(!below.exceeded_limit);
    assert_eq!(below.retained_bytes, MAX_ROLLUP_CHANGE_REPORT_BYTES - 1);
    assert!(!exact.exceeded_limit);
    assert_eq!(exact.retained_bytes, MAX_ROLLUP_CHANGE_REPORT_BYTES);
    assert!(above.exceeded_limit);
    assert!(above.rows.is_empty());
    assert_eq!(above.retained_bytes, 0);
}

fn report_with_partition_bytes(bytes: usize) -> RollupSourceChangeReport {
    let fields = vec!["d".to_owned()];
    let payload = json!({ "d": "x".repeat(bytes) });
    let mut report = RollupSourceChangeReport::default();
    report.record(None, Some(&payload), &fields);
    report
}
