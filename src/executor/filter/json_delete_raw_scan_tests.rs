use super::{scan, scan_observing, scan_prefix, Progress};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use serde_json::value::RawValue;
use std::time::Instant;

fn controls(cancel: QueryCancellationHandle) -> QueryExecutionControls {
    QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel,
    )
}

#[test]
fn should_match_raw_serde_syntax_without_decoding_surrogates() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let deep = format!("{}0{}", "[".repeat(160), "]".repeat(160));
    let inputs = [
        r#"{"drop":"\ud800","ok":1}"#,
        r#"{"\ud800":0}"#,
        r#"["\u0000",true,false,null,-1.2e+3]"#,
        deep.as_str(),
        "[1,]",
        "{\"x\":}",
        "{\"x\":1,}",
        "[1 2]",
        "1.",
        "1.\n",
        "1e",
        "01",
        "\"\\q\"",
        r#""\uX123""#,
        "\"\\uX",
        "\"a\u{0001}b\"",
        "[",
        "{} trailing",
    ];
    // Act / Assert
    for input in inputs {
        let expected = serde_json::from_str::<&RawValue>(input);
        let actual = scan(input, &controls);
        // Assert
        assert_eq!(actual.is_ok(), expected.is_ok(), "{input:?}");
        if let (Err(actual), Err(expected)) = (actual, expected) {
            assert_eq!(actual.to_string(), expected.to_string(), "{input:?}");
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_raw_scan_stack_after_actual_input_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let input = format!("{}0{}", "[".repeat(200), "]".repeat(200));
    let mut advanced = 0;
    // Act
    let result = scan_observing(&input, &controls, |progress| {
        if matches!(progress, Progress::Input) {
            advanced += 1;
            if advanced == 64 {
                cancellation.cancel();
            }
        }
    });
    // Assert
    assert_eq!(advanced, 64);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_old_with_new_stacks_after_actual_move_cancellation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    let input = format!("{}0{}", "[".repeat(32), "]".repeat(32));
    let mut moved = 0;
    // Act
    let result = scan_observing(&input, &controls, |progress| {
        if matches!(progress, Progress::StackMove) {
            moved += 1;
            cancellation.cancel();
        }
    });
    // Assert
    assert_eq!(moved, 1);
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_the_measured_raw_scan_stack_overlap_budget() {
    // Arrange
    let input = format!("{}0{}", "[".repeat(32), "]".repeat(32));
    let baseline = controls(QueryCancellationHandle::new());
    scan(&input, &baseline).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let limited = QueryExecutionControls::from_limits(&limits, Instant::now());
        let result = scan(&input, &limited);
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_return_the_exact_borrowed_value_span_before_following_input() {
    // Arrange
    let controls = controls(QueryCancellationHandle::new());
    let input = "  [1,{\"x\":\"\\ud800\"}] , next";
    // Act
    let span = scan_prefix(input, &controls).expect("one raw value");
    // Assert
    assert_eq!(&input[span], r#"[1,{"x":"\ud800"}]"#);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_honor_cancellation_before_empty_or_invalid_input() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = controls(cancellation.clone());
    cancellation.cancel();
    // Act / Assert
    for input in ["", "?"] {
        let result = scan(input, &controls);
        // Assert
        assert!(matches!(
            result,
            Err(super::ScanError::Control(
                crate::app::CassieError::QueryCancelled
            ))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
