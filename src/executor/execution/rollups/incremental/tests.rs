use serde_json::json;

use super::{MAX_AFFECTED_GROUPS, affected_groups};
use crate::catalog::{RollupDefinition, RollupMeta};
use crate::midge::adapter::documents::RollupSourceChangeReport;

#[test]
fn should_fallback_when_a_bounded_change_report_targets_more_than_256_groups() {
    // Arrange
    let meta = RollupMeta::new(RollupDefinition {
        name: "postgres.public.events_daily".to_owned(),
        source_collection: "postgres.public.events".to_owned(),
        timestamp_field: "event_at".to_owned(),
        bucket_width: "1 day".to_owned(),
        origin: None,
        bucket_expr: "time_bucket('1 day', event_at)".to_owned(),
        group_keys: vec!["device".to_owned()],
        aggregates: Vec::new(),
        filter_expr: None,
    });
    let fields = vec!["device".to_owned(), "event_at".to_owned()];
    let mut report = RollupSourceChangeReport::default();
    for index in 0..=MAX_AFFECTED_GROUPS {
        let payload = json!({
            "device": format!("{index:03}"),
            "event_at": "2026-01-01T01:00:00Z"
        });
        report.record(&index.to_string(), None, Some(&payload), &fields);
    }

    // Act
    let groups = affected_groups(&report, &meta);

    // Assert
    assert!(!report.exceeded_limit);
    assert_eq!(report.rows.len(), MAX_AFFECTED_GROUPS + 1);
    assert!(groups.is_none());
}
