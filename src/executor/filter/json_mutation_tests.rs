use super::{evaluate, Value};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

#[test]
fn should_admit_json_formatting_memory_given_small_limits_when_pretty_output_is_constructed() {
    // Arrange
    let input = Value::String("{\"a\":[1,null]}".into());
    let scarce = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let ample = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 65536,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    // Act
    let denied = evaluate("jsonb_pretty", std::slice::from_ref(&input), Some(&scarce))
        .expect("pretty function");
    let accepted = evaluate("jsonb_pretty", &[input], Some(&ample)).expect("pretty function");
    // Assert
    assert!(denied
        .expect_err("formatting exceeds budget")
        .to_string()
        .contains("memory budget exceeded"));
    assert_eq!(
        accepted.expect("admitted formatting"),
        Value::String("{\n    \"a\": [\n        1,\n        null\n    ]\n}".into())
    );
    assert_eq!(scarce.current_query_memory_bytes(), 0);
    assert_eq!(ample.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_json_mutation_memory_given_path_errors_and_cancellation_when_queries_stop() {
    // Arrange
    let limits = CassieRuntimeLimits::default();
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let cancellation = QueryCancellationHandle::new();
    let cancelled =
        QueryExecutionControls::with_cancellation(&limits, Instant::now(), cancellation.clone());
    cancellation.cancel();
    let args = [
        Value::String("[]".into()),
        Value::String("{bad}".into()),
        Value::String("2".into()),
    ];
    // Act
    let invalid = evaluate("jsonb_set", &args, Some(&controls)).expect("mutation function");
    let interrupted = evaluate("jsonb_set", &args, Some(&cancelled)).expect("mutation function");
    // Assert
    assert!(invalid.is_err());
    assert!(interrupted
        .expect_err("cancelled mutation")
        .to_string()
        .contains("cancel"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
}
