use super::{parse, parse_observing};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

#[test]
fn should_release_value_arena_after_actual_parse_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let mut frames = 0;
    // Act
    let result = parse_observing(r#"{"a":[1,2]}"#, &controls, || {
        frames += 1;
        cancel.cancel();
    });
    // Assert
    assert_eq!(frames, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_value_arena_after_actual_element_lookup_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let document = parse(r#"["first","second"]"#, &controls)
        .expect("parse")
        .expect("document");
    let mut visited = 0;
    // Act
    let result =
        document
            .parsed
            .forest
            .key_exists_observing(document.parsed.root, "second", || {
                visited += 1;
                cancel.cancel();
            });
    // Assert
    assert_eq!(visited, 1);
    assert!(result.is_err());
    drop(document);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_measured_value_arena_overlap_budget() {
    // Arrange
    let document = r#"{"a":["first","second"],"b":"payload"}"#;
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let value = parse(document, &baseline)
        .expect("parse")
        .expect("document");
    // Assert
    assert!(value.contains("a").expect("lookup"));
    let peak = baseline.peak_query_memory_bytes();
    drop(value);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = parse(document, &controls);
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
