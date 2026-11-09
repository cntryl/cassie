//! Deferred controlled root-inspection probes.
use super::{evaluate, Value};
use crate::config::CassieRuntimeLimits;
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
    input: Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, String> {
    evaluate(name, &[input], controls)
        .expect("known function")
        .map_err(|error| error.to_string())
}

#[test]
fn should_match_legacy_root_kind_array_length_sentinel_and_error_behavior() {
    // Arrange
    let values = [
        Value::String("null".into()),
        Value::String("true".into()),
        Value::String("1.20".into()),
        Value::String(r#""text""#.into()),
        Value::String("[]".into()),
        Value::String("[1,2]".into()),
        Value::String(r#"{"a":1}"#.into()),
        Value::String("[}".into()),
        Value::String(r#"{"$serde_json::private::Number":"7"}"#.into()),
        Value::String(r#"{"$serde_json::private::RawValue":"[1,2]"}"#.into()),
        Value::Json(serde_json::json!([1, 2])),
        Value::Null,
    ];
    let controls = controls(16 * 1024 * 1024);
    // Act / Assert
    for name in [
        "json_typeof",
        "jsonb_typeof",
        "json_array_length",
        "jsonb_array_length",
    ] {
        for input in &values {
            assert_eq!(
                run(name, input.clone(), Some(&controls)),
                run(name, input.clone(), None),
                "{name} {input:?}"
            );
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_release_inspection_scratch_and_arena_after_actual_parse_cancellation() {
    // Arrange
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    crate::runtime::accounted::json_arena::cancel_existence_parse_once(cancel.clone());
    // Act
    let result = run(
        "jsonb_typeof",
        Value::String(r#"{"a":[1,2]}"#.into()),
        Some(&controls),
    );
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_inspection_scratch_and_arena_overlap() {
    // Arrange
    let input = Value::String(r#"[{"a":1},{"b":2}]"#.into());
    let baseline = controls(16 * 1024 * 1024);
    assert!(matches!(
        run("jsonb_array_length", input.clone(), Some(&baseline)),
        Ok(Value::Int64(2))
    ));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        assert_eq!(
            run("jsonb_array_length", input.clone(), Some(&controls)).is_ok(),
            succeeds
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
