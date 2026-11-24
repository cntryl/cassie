use std::time::Instant;

use super::super::super::document::Document;
use super::super::jsonb_order::sorted;
use super::super::Kind;
use super::{serialize, serialize_observed, OutputProgress, SerializeError};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

#[test]
fn should_match_jsonb_canonical_values_with_byte_length_key_order() {
    // Arrange
    let controls = controls();
    let inputs = [
        "null",
        "true",
        "-0",
        "1E2",
        "[]",
        "{}",
        r#"{"abc":1,"z":2,"é":3,"aa":4,"a":0,"a":5}"#,
        r#"{"z":[null,true,"\b\f\n\r\t\u0001\\\"é🙂"],"a":1.2300}"#,
        r#"{"$serde_json::private::RawValue":"{\"λ\":[1,2]}"}"#,
        r#"{"éa":1,"é":2,"aa":3,"e\u0301":4,"a":5,"🙂":6,"ab":7}"#,
    ];

    // Act / Assert
    for input in inputs {
        let expected = crate::types::jsonb::canonical(input).expect("canonical");
        let document = Document::parse_for_output(input, &controls).expect("document");
        let actual = serialize(&document.forest, document.root).expect("output");
        // Assert
        assert_eq!(actual.as_str(), expected, "{input}");
        drop(actual);
        drop(document);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_both_sort_buffers_at_the_exact_overlap_boundary() {
    fn check(allowance: usize, expected: bool) {
        let limit = 65_536;
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: limit,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let document =
            Document::parse_for_output(r#"{"a":1,"b":2,"c":3}"#, &controls).expect("document");
        let baseline = controls.current_query_memory_bytes();
        let blocker = controls
            .reserve_query_memory(limit - baseline - allowance)
            .expect("blocker");
        let Kind::Object(index) = &document.forest.nodes.get(document.root).expect("root").kind
        else {
            panic!("object")
        };
        let result = sorted(index, &controls, &mut || {});
        if expected {
            // Assert
            assert!(result.is_ok());
        } else {
            assert!(matches!(&result, Err(CassieError::ResourceLimit(_))));
        }
        drop(result);
        drop(blocker);
        drop(document);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    // Arrange
    let exact = 2 * 3 * std::mem::size_of::<usize>();
    // Act / Assert
    check(exact, true);
    check(exact - 1, false);
}

#[test]
fn should_release_nested_order_with_frames_when_child_sort_is_canceled() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let document =
        Document::parse_for_output(r#"{"a":{"x":1,"y":2},"b":0}"#, &controls).expect("document");
    let baseline = controls.current_query_memory_bytes();
    let mut comparisons = 0;
    // Act
    let result = serialize_observed(&document.forest, document.root, |progress| {
        if matches!(progress, OutputProgress::SortCompare) {
            comparisons += 1;
            if comparisons == 2 {
                cancellation.cancel();
            }
        }
    });
    // Assert
    assert_eq!(comparisons, 2);
    assert!(matches!(
        result,
        Err(SerializeError::Control(CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(document);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_source_output_with_workspace_on_string_with_numeric_cancellation() {
    for (input, phase) in [
        (format!("\"{}\"", "x".repeat(2048)), 0),
        ("0e-1500".to_owned(), 1),
        (format!("\"{}\"", "x".repeat(2048)), 2),
        (format!("1.{}", "0".repeat(2048)), 3),
    ] {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let document = Document::parse_for_output(&input, &controls).expect("document");
        let baseline = controls.current_query_memory_bytes();
        let mut observed = false;
        // Act
        let result = serialize_observed(&document.forest, document.root, |progress| {
            if matches!(
                (phase, progress),
                (0, OutputProgress::StringScan)
                    | (1, OutputProgress::NumericOutput)
                    | (2, OutputProgress::SinkCopy)
                    | (3, OutputProgress::NumericScan)
            ) {
                observed = true;
                cancellation.cancel();
            }
        });
        // Assert
        assert!(observed);
        assert!(matches!(
            result,
            Err(SerializeError::Control(CassieError::QueryCanceled))
        ));
        assert_eq!(controls.current_query_memory_bytes(), baseline);
        drop(document);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_report_first_sorted_key_or_value_failure_with_discard_overwritten_values() {
    // Arrange
    let controls = controls();
    let inputs = [
        r#"{"b":"\u0000","a":1e+131072}"#,
        r#"{"long":1e+131072,"a":"\u0000"}"#,
        r#"{"a\u0000":1e+131072}"#,
        r#"{"a":"\u0000","a":3}"#,
    ];

    // Act / Assert
    for input in inputs {
        let expected = crate::types::jsonb::canonical(input);
        let document = Document::parse_for_output(input, &controls).expect("document");
        let result = serialize(&document.forest, document.root);
        match (expected, result) {
            // Assert
            (Ok(expected), Ok(actual)) => assert_eq!(actual.as_str(), expected),
            (Err(expected), Err(actual)) => assert_eq!(
                (actual.sqlstate(), actual.message()),
                (expected.sqlstate, expected.message.as_str())
            ),
            _ => panic!("different JSONB error priority for {input}"),
        }
        drop(document);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_during_checked_sort_comparison_with_release_workspaces() {
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
    let document = Document::parse_for_output(&input, &controls).expect("document");
    let baseline = controls.current_query_memory_bytes();
    let mut compared = false;

    // Act
    let result = serialize_observed(&document.forest, document.root, |progress| {
        if matches!(progress, OutputProgress::SortCompare) {
            compared = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(compared);
    assert!(matches!(
        result,
        Err(SerializeError::Control(CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), baseline);
    drop(document);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
