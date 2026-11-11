use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::collections::HashMap;
use std::time::Instant;

fn input() -> Vec<Vec<Expr>> {
    vec![vec![crate::sql::parser::parse_expression(
        r#"'{"drop":1,"keep":"payload"}'::JSONB - 'drop'"#,
    )
    .expect("expression")]]
}

#[test]
fn should_hold_values_deletion_output_through_source_row_ownership() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    // Act
    let (rows, _) = execute(&env, &input(), None).expect("VALUES");
    // Assert
    assert!(
        matches!(rows[0][0].get("column1"), Some(Value::String(text)) if text == r#"{"keep": "payload"}"#)
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_values_deletion_owner_when_canceled_before_output_transfer() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    super::cancel_after_value_once(cancel.clone());
    // Act
    let result = execute(&env, &input(), None);
    // Assert
    assert!(cancel.is_canceled());
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_values_deletion_overlap_budget() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &baseline,
    };
    let (rows, _) = execute(&env, &input(), None).expect("baseline");
    let peak = baseline.peak_query_memory_bytes();
    drop(rows);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let env = SourceExecutionEnv {
            controls: &controls,
            ..env
        };
        let result = execute(&env, &input(), None);
        // Assert
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
