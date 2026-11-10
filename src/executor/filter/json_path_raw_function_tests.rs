//! Deferred function-level checked raw path probes.
use super::{evaluate, Value};
use crate::config::CassieRuntimeLimits;
use crate::executor::filter::EvaluationMemory;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
fn run(
    name: &str,
    args: &[Value],
    controls: Option<&QueryExecutionControls>,
    owner: Option<&EvaluationMemory>,
) -> Result<Value, String> {
    evaluate(name, args, controls, owner)
        .expect("known function")
        .map_err(|error| error.to_string())
}

#[test]
fn should_match_legacy_finite_path_functions_and_error_priority() {
    // Arrange
    let cases = [
        vec![
            Value::String(r#"{"a":{"b":"v"}}"#.into()),
            Value::String("a".into()),
            Value::String("b".into()),
        ],
        vec![Value::String("[1,2,3]".into()), Value::String("-1".into())],
        vec![
            Value::String(r#"{"a":null}"#.into()),
            Value::String("a".into()),
        ],
        vec![
            Value::String(r#"{"a":1}"#.into()),
            Value::String("missing".into()),
            Value::Int64(9),
        ],
        vec![Value::String("[}".into()), Value::Int64(9)],
        vec![Value::String("[}".into()), Value::Null],
        vec![
            Value::String(r#"{"a":"\ud800"}"#.into()),
            Value::String("a".into()),
        ],
        vec![
            Value::String(r#"{"a":1e131072}"#.into()),
            Value::String("a".into()),
        ],
        vec![Value::String("{}".into())],
    ];
    // Act / Assert
    for name in [
        "json_extract_path",
        "json_extract_path_text",
        "jsonb_extract_path",
        "jsonb_extract_path_text",
    ] {
        for args in &cases {
            let controls = controls(32 * 1024 * 1024);
            let owner = EvaluationMemory::new(&controls).expect("owner");
            let expected = run(name, args, Some(&controls), None);
            let actual = run(name, args, Some(&controls), Some(&owner));
            assert_eq!(actual, expected, "{name} {args:?}");
            drop(owner);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_release_workspace_after_actual_function_document_scan_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    super::super::super::json_delete::cancel_function_path_scan_once(cancel.clone());
    let args = [
        Value::String(r#"{"a":"payload"}"#.into()),
        Value::String("a".into()),
    ];
    // Act
    let result = run(
        "json_extract_path_text",
        &args,
        Some(&controls),
        Some(&owner),
    );
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_both_function_output_and_workspace_after_actual_copy_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let owner = EvaluationMemory::new(&controls).expect("owner");
    super::super::super::json_delete::cancel_function_path_copy_once(cancel.clone());
    let args = [
        Value::String(format!(r#"{{"a":"{}"}}"#, "x".repeat(2_048))),
        Value::String("a".into()),
    ];
    // Act
    let result = run(
        "json_extract_path_text",
        &args,
        Some(&controls),
        Some(&owner),
    );
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_function_workspace_and_output_overlap() {
    // Arrange
    let args = [
        Value::String(r#"{"a":"payload"}"#.into()),
        Value::String("a".into()),
    ];
    let baseline = controls(32 * 1024 * 1024);
    let owner = EvaluationMemory::new(&baseline).expect("owner");
    let value = run(
        "json_extract_path_text",
        &args,
        Some(&baseline),
        Some(&owner),
    )
    .expect("result");
    let peak = baseline.peak_query_memory_bytes();
    drop(value);
    drop(owner);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let owner = EvaluationMemory::new(&controls).expect("owner");
        let result = run(
            "json_extract_path_text",
            &args,
            Some(&controls),
            Some(&owner),
        );
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        drop(owner);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_preserve_foreign_owner_diagnostic_and_release_both_trackers() {
    // Arrange
    let source_controls = controls(10 * 1024 * 1024);
    let foreign_controls = controls(10 * 1024 * 1024);
    let owner = EvaluationMemory::new(&source_controls).expect("owner");
    let args = [
        Value::String(r#"{"a":1}"#.into()),
        Value::String("a".into()),
    ];
    // Act
    let error = run(
        "json_extract_path",
        &args,
        Some(&foreign_controls),
        Some(&owner),
    )
    .expect_err("foreign owner");
    // Assert
    assert!(error.contains("JSON path owner uses a different query tracker"));
    assert_eq!(foreign_controls.current_query_memory_bytes(), 0);
    drop(owner);
    assert_eq!(source_controls.current_query_memory_bytes(), 0);
}
