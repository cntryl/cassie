use std::cell::Cell;
use std::collections::HashMap;
use std::time::Instant;

use super::{
    accounting, execute_vectorized_join, BatchRow, EquiJoinKeys, JoinKind, JoinRetentionContext,
    JoinRetentionPhase, NumericComparisonDomain, SourceExecutionEnv, VectorizedJoinAction,
    VectorizedJoinOutcome, VectorizedJoinSpec,
};
use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled, OperatorSwitchingEnabled};
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

struct Observation {
    denied: bool,
    output_rows: Option<usize>,
    build_calls: usize,
    admitted_before_build: bool,
    peak: usize,
    retired: usize,
}

fn run_build(
    cassie: &Cassie,
    rows: &[BatchRow],
    key: &str,
    required: usize,
    budget: usize,
) -> Observation {
    let mut limits = cassie.runtime.limits();
    limits.query_memory_budget_bytes = budget;
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let keys = EquiJoinKeys {
        left: "unused".into(),
        right: key.into(),
        domain: NumericComparisonDomain::Float8,
    };
    let calls = Cell::new(0);
    let admitted = Cell::new(true);
    let probe = |phase| {
        if phase == JoinRetentionPhase::HashBuild {
            calls.set(calls.get() + 1);
            admitted.set(admitted.get() && controls.current_query_memory_bytes() == required);
        }
        Ok(())
    };
    let template = BatchRow::new(Vec::new());
    let result = execute_vectorized_join(
        &env,
        VectorizedJoinSpec {
            kind: JoinKind::Inner,
            sources: None,
            keys: &keys,
            left_rows: &[],
            right_rows: rows,
            right_template: &template,
            row_budget: None,
        },
        &JoinRetentionContext::with_probe(&probe),
    )
    .map_err(CassieError::from);
    let mut denied = false;
    let output_rows = match result {
        Ok(VectorizedJoinOutcome {
            action: VectorizedJoinAction::Executed,
            joined: Some(joined),
        }) => Some(joined.rows.len()),
        Ok(_) => None,
        Err(error) => {
            denied = matches!(error, CassieError::ResourceLimit(_));
            None
        }
    };
    Observation {
        denied,
        output_rows,
        build_calls: calls.get(),
        admitted_before_build: admitted.get(),
        peak: controls.peak_query_memory_bytes(),
        retired: controls.current_query_memory_bytes(),
    }
}

#[test]
fn should_admit_normalized_hash_build_keys_at_the_measured_boundary() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-numeric-key-admission-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 64 * 1024;
    config.limits.vectorized_joins_enabled = true;
    config.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie =
        Cassie::new_with_data_dir_and_config(&path, config).expect("key admission fixture");
    // Long existing key names keep the measured key build above the selector's conservative floor.
    let key = "k".repeat(64);
    let rows = [9_007_199_254_740_993, 9_007_199_254_740_992]
        .map(|value| BatchRow::new(vec![(key.clone(), Value::Int64(value))]));
    let required =
        accounting::hash_build_bytes::<&BatchRow>(&rows, &key, NumericComparisonDomain::Float8)
            .expect("measured Float8 build");
    let exact =
        accounting::hash_build_bytes::<&BatchRow>(&rows, &key, NumericComparisonDomain::Exact)
            .expect("measured exact build");

    // Act
    let denied = run_build(&cassie, &rows, &key, required, required - 1);
    let admitted = run_build(&cassie, &rows, &key, required, required);
    let carriers = rows
        .iter()
        .map(|row| row.entries()[0].1.clone())
        .collect::<Vec<_>>();
    drop(rows);
    drop(cassie);
    let cleanup = std::fs::remove_dir_all(&path);

    // Assert
    cleanup.expect("retire owned fixture");
    assert_eq!(
        required - exact,
        2 * 64,
        "both integer keys require normalization scratch"
    );
    assert!(required > super::estimate_vectorized_join_bytes(0, 2));
    assert!(denied.denied);
    assert_eq!(
        denied.build_calls, 0,
        "denial precedes actual key construction"
    );
    assert_eq!(denied.retired, 0);
    assert_eq!(
        admitted.output_rows,
        Some(0),
        "empty probe still builds both keys"
    );
    assert_eq!(admitted.build_calls, 2);
    assert!(admitted.admitted_before_build);
    assert_eq!(admitted.peak, required);
    assert_eq!(admitted.retired, 0);
    assert_eq!(
        carriers,
        vec![
            Value::Int64(9_007_199_254_740_993),
            Value::Int64(9_007_199_254_740_992)
        ]
    );
}
