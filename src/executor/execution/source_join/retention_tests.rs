use std::cell::Cell;
use std::collections::HashMap;
use std::time::Instant;

use crate::app::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryExecutionControls;

use super::{
    execute_loaded_join_with_context, reserve_join_rows, BatchRow, BinaryOp, Expr, JoinKind,
    JoinResult, JoinRetentionContext, JoinRetentionPhase, JoinRowsSpec, QueryError,
    SourceExecutionEnv, Value,
};

#[test]
fn should_admit_cross_output_before_constructing_payload_copies() {
    // Arrange
    let phase = JoinRetentionPhase::NestedOutput;

    // Act
    let result = probe_loaded_phase(phase);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_admit_hash_build_before_constructing_a_retained_text_key() {
    // Arrange
    let phase = JoinRetentionPhase::HashBuild;

    // Act
    let result = probe_loaded_phase(phase);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_admit_hash_output_before_constructing_payload_copies() {
    // Arrange
    let phase = JoinRetentionPhase::HashOutput;

    // Act
    let result = probe_loaded_phase(phase);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_admit_merge_keyed_state_before_cloning_a_retained_row() {
    // Arrange
    let phase = JoinRetentionPhase::MergeKeyed;

    // Act
    let result = probe_loaded_phase(phase);

    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
}

fn probe_loaded_phase(phase: JoinRetentionPhase) -> Result<JoinResult, CassieError> {
    let path = std::env::temp_dir().join(format!(
        "cassie-join-retention-phase-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 20 * 1024;
    config.limits.vectorized_joins_enabled = phase != JoinRetentionPhase::MergeKeyed;
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("controlled Cassie");
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let text_key = phase == JoinRetentionPhase::HashBuild;
    let rows = |side: &str| {
        let key = if text_key {
            Value::String("k".repeat(8192))
        } else {
            Value::Int64(1)
        };
        let mut entries = vec![(format!("{side}.key"), key)];
        if !text_key {
            entries.push((format!("{side}.payload"), Value::String("p".repeat(8192))));
        }
        [BatchRow::new(entries)]
    };
    let left = rows("left");
    let right = rows("right");
    let left_template = BatchRow::new(vec![("left.key".to_owned(), Value::Null)]);
    let right_template = BatchRow::new(vec![("right.key".to_owned(), Value::Null)]);
    let on = Expr::Binary {
        left: Box::new(Expr::Column("left.key".to_owned())),
        op: BinaryOp::Eq,
        right: Box::new(Expr::Column("right.key".to_owned())),
    };
    let input_memory = reserve_join_rows(&env, &left, &right).expect("complete inputs fit");
    let input_bytes = controls.current_query_memory_bytes();
    assert!(input_bytes >= 2 * 8192);
    assert!(input_bytes < controls.query_memory_budget_bytes);
    assert!(!controls.is_cancelled());
    assert!(controls.deadline.is_none());
    let canary_calls = Cell::new(0);
    let canary = |selected| {
        if selected == phase {
            assert!(controls.current_query_memory_bytes() >= input_bytes);
            canary_calls.set(canary_calls.get() + 1);
            return Err(QueryError::from(CassieError::Execution(format!(
                "unguarded join constructor reached: {phase:?}"
            ))));
        }
        Ok(())
    };
    let retention = JoinRetentionContext::with_probe(&canary);
    let before = cassie.runtime.snapshot();
    let result = execute_loaded_join_with_context(
        &env,
        JoinRowsSpec {
            predicate_context: None,
            sources: None,
            kind: if phase == JoinRetentionPhase::NestedOutput {
                JoinKind::Cross
            } else {
                JoinKind::Inner
            },
            on: &on,
            left_rows: &left,
            right_rows: &right,
            left_template: &left_template,
            right_template: &right_template,
            row_budget: Some(1),
        },
        &["left.key".to_owned()],
        &["right.key".to_owned()],
        || {},
        &retention,
    )
    .map_err(CassieError::from);
    let after = cassie.runtime.snapshot();
    assert_eq!(after.joins.executions, before.joins.executions);
    assert_eq!(
        after.adaptive_candidates.operator_switch_successes,
        before.adaptive_candidates.operator_switch_successes
    );
    assert_eq!(controls.current_query_memory_bytes(), input_bytes);
    drop(input_memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    // Report the exact phase canary on the unfixed path; an admitted implementation must reject
    // the oversized constructor before invoking it.
    assert_eq!(
        canary_calls.get(),
        usize::from(result.is_err() && !matches!(result, Err(CassieError::ResourceLimit(_))))
    );
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
    result
}
