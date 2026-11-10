use std::time::Instant;

use super::super::super::document::Document;
use super::{serialize, serialize_observed, Frame, OutputProgress};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_plain_value_compact_output_for_arena_values() {
    // Arrange
    let controls = controls();
    let inputs = [
        "null",
        "true",
        "-0",
        "1E2",
        "[]",
        "{}",
        r#"{"z":[null,true,"\b\f\n\r\t\u0001\\\"é🙂"],"a":1,"a":2}"#,
        r#"{"é":1,"e\u0301":2,"":3,"e":4,"🙂":5,"z":6}"#,
        r#"{"$serde_json::private::Number":"1E2"}"#,
        r#"{"$serde_json::private::RawValue":"{\"λ\":[1,2]}"}"#,
    ];

    // Act / Assert
    for input in inputs {
        let expected = {
            let parsed =
                crate::runtime::accounted::json_arena::parse_value(input.as_bytes(), &controls)
                    .expect("ordinary JSON");
            parsed.get().to_string()
        };
        let parsed = Document::parse_for_output(input, &controls).expect("document");
        let output = serialize(&parsed.forest, parsed.root).expect("output");
        // Assert
        assert_eq!(output.as_str(), expected, "{input}");
        drop(output);
        drop(parsed);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_output_with_frame_capacity_while_forest_remains_owned() {
    fn check(allowance: usize, succeeds: bool) {
        let limit = 16384;
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: limit,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let parsed = Document::parse_for_output("null", &controls).expect("document");
        let baseline = controls.current_query_memory_bytes();
        let blocker = controls
            .reserve_query_memory(limit - baseline - allowance)
            .expect("blocker");
        let result = serialize(&parsed.forest, parsed.root);
        if succeeds {
            let output = result.expect("exact");
            // Assert
            assert_eq!(output.as_str(), "null");
            drop(output);
        } else {
            assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
        }
        drop(blocker);
        drop(parsed);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    // Arrange
    let exact = 4 * std::mem::size_of::<Frame>() + 8;

    // Act / Assert
    check(exact, true);
    check(exact - 1, false);
}

#[test]
fn should_release_output_with_stack_when_canceled_during_key_comparison() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let left = format!("{}a", "x".repeat(1024));
    let right = format!("{}b", "x".repeat(1024));
    let input = format!("{{\"{left}\":1,\"{right}\":2}}");
    let parsed = Document::parse_for_output(&input, &controls).expect("document");
    let baseline = controls.current_query_memory_bytes();
    let mut compared = false;

    // Act
    let result = serialize_observed(&parsed.forest, parsed.root, |progress| {
        if matches!(progress, OutputProgress::KeyCompare) {
            compared = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(compared);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_both_output_buffers_after_real_growth_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let parsed = Document::parse_for_output("\"abcdefghijklmnop\"", &controls).expect("document");
    let baseline = controls.current_query_memory_bytes();
    let overlap = baseline + 4 * std::mem::size_of::<Frame>() + 8 + 32;
    let mut saw_overlap = false;

    // Act
    let result = serialize_observed(&parsed.forest, parsed.root, |progress| {
        if matches!(progress, OutputProgress::SinkCopy)
            && controls.current_query_memory_bytes() == overlap
        {
            saw_overlap = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(saw_overlap);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_traversal_frames_when_canceled_during_actual_stack_growth() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let parsed = Document::parse_for_output("[[[[[null]]]]]", &controls).expect("document");
    let baseline = controls.current_query_memory_bytes();
    let mut moved = false;

    // Act
    let result = serialize_observed(&parsed.forest, parsed.root, |progress| {
        if matches!(progress, OutputProgress::FrameCopy) {
            moved = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(moved);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_output_when_canceled_during_actual_unicode_scan() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let input = format!("\"{}\"", "é".repeat(513));
    let parsed = Document::parse_for_output(&input, &controls).expect("document");
    let baseline = controls.current_query_memory_bytes();
    let mut scanned = false;

    // Act
    let result = serialize_observed(&parsed.forest, parsed.root, |progress| {
        if matches!(progress, OutputProgress::SinkScan) {
            scanned = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(scanned);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(parsed);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
