use std::cell::Cell;
use std::collections::HashMap;
use std::time::Instant;

use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, OperatorSwitchingEnabled};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

use super::{
    execute_loaded_join_with_replacement_probe, reserve_join_rows, BatchRow, BinaryOp, Expr,
    JoinKind, JoinRowsSpec, SourceExecutionEnv, Value, VECTOR_TO_MERGE_SWITCH_PAIR,
};

#[test]
fn should_cancel_an_activated_merge_replacement_before_publishing_success() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-activated-merge-controls-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.vectorized_joins_enabled = true;
    config.limits.operator_switching_enabled = OperatorSwitchingEnabled::enabled();
    config.limits.operator_switch_join_row_threshold = 1;
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("controlled Cassie");
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &config.limits,
        Instant::now(),
        cancellation.clone(),
    );
    assert!(controls.deadline.is_none());
    assert!(!controls.is_cancelled());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let left = [BatchRow::new(vec![(
        "left.key".to_owned(),
        Value::Int64(1),
    )])];
    let right = [BatchRow::new(vec![(
        "right.key".to_owned(),
        Value::Int64(1),
    )])];
    let left_template = BatchRow::new(vec![("left.key".to_owned(), Value::Null)]);
    let right_template = BatchRow::new(vec![("right.key".to_owned(), Value::Null)]);
    let on = Expr::Binary {
        left: Box::new(Expr::Column("left.key".to_owned())),
        op: BinaryOp::Eq,
        right: Box::new(Expr::Column("right.key".to_owned())),
    };
    let input_memory = reserve_join_rows(&env, &left, &right).expect("input reservation");
    let input_bytes = controls.current_query_memory_bytes();
    let probe_calls = Cell::new(0);
    let before = cassie.runtime.snapshot();

    // Act
    let result = execute_loaded_join_with_replacement_probe(
        &env,
        JoinRowsSpec {
            sources: None,
            kind: JoinKind::Inner,
            on: &on,
            left_rows: &left,
            right_rows: &right,
            left_template: &left_template,
            right_template: &right_template,
            row_budget: None,
        },
        &["left.key".to_owned()],
        &["right.key".to_owned()],
        || {
            assert!(!controls.is_cancelled());
            assert!(controls.current_query_memory_bytes() > input_bytes);
            probe_calls.set(probe_calls.get() + 1);
            cancellation.cancel();
        },
    )
    .map_err(CassieError::from);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(probe_calls.get(), 1, "actual replacement must be activated");
    let after = cassie.runtime.snapshot();
    assert_eq!(
        after.adaptive_candidates.operator_switch_successes,
        before.adaptive_candidates.operator_switch_successes
    );
    assert_eq!(
        after.adaptive_candidates.operator_switch_fallbacks,
        before.adaptive_candidates.operator_switch_fallbacks + 1
    );
    assert_eq!(
        after.adaptive_candidates.last_operator_switch_pair,
        VECTOR_TO_MERGE_SWITCH_PAIR
    );
    assert_eq!(
        after.adaptive_candidates.last_operator_switch_reason,
        "replacement_failed"
    );
    assert_eq!(
        after.adaptive_candidates.last_operator_switch_state,
        "replay_left_rows=1;replay_right_rows=1;rows_emitted=0"
    );
    assert_eq!(after.joins.executions, before.joins.executions);
    assert_eq!(controls.current_query_memory_bytes(), input_bytes);
    drop(input_memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
