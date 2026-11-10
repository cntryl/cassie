//! Deferred function-level controlled existence probes.
use super::{evaluate, Value};
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::SqlArray;
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
) -> Result<Value, String> {
    evaluate(name, args, controls)
        .expect("known function")
        .map_err(|error| error.to_string())
}

#[test]
fn should_match_legacy_existence_function_results_with_errors() {
    // Arrange
    let native = Value::Array(
        SqlArray::from_elements(vec![Value::String("a".into()), Value::Null]).expect("array"),
    );
    let cases = [
        vec![
            Value::String(r#"{"a":1,"b":2}"#.into()),
            Value::String("{a,b}".into()),
        ],
        vec![Value::String(r#"["a","b",1]"#.into()), native],
        vec![Value::String(r#""a""#.into()), Value::String("{}".into())],
        vec![Value::String("null".into()), Value::String("{NULL}".into())],
        vec![
            Value::Json(serde_json::json!({"$serde_json::private::Number":"7"})),
            Value::String("{a}".into()),
        ],
        vec![Value::String("[\u{7d}".into()), Value::Int64(9)],
        vec![Value::String("{}".into()), Value::String("{a,}".into())],
        vec![Value::Null, Value::String("{a}".into())],
        vec![Value::String("{}".into()), Value::Null],
    ];
    let controls = controls(32 * 1024 * 1024);
    // Act
    for name in ["jsonb_exists_any", "jsonb_exists_all"] {
        for args in &cases {
            // Assert
            assert_eq!(
                run(name, args, Some(&controls)),
                run(name, args, None),
                "{name} {args:?}"
            );
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}

#[test]
fn should_release_function_scratch_document_with_path_after_actual_path_cancellation() {
    // Arrange
    let args = [
        Value::String(r#"{"a":1,"b":2}"#.into()),
        Value::String("{a,b}".into()),
    ];
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    crate::executor::filter::json_delete::cancel_after_owned_path_once(cancel.clone());
    // Act
    let result = run("jsonb_exists_all", &args, Some(&controls));
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_function_scratch_arena_with_path_overlap() {
    // Arrange
    let args = [
        Value::String(r#"{"a":1,"b":2}"#.into()),
        Value::String("[4:5]={a,b}".into()),
    ];
    let baseline = controls(32 * 1024 * 1024);
    // Assert
    assert!(matches!(
        run("jsonb_exists_all", &args, Some(&baseline)),
        Ok(Value::Bool(true))
    ));
    let peak = baseline.peak_query_memory_bytes();
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        assert_eq!(
            run("jsonb_exists_all", &args, Some(&controls)).is_ok(),
            succeeds
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
