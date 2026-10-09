use std::collections::HashMap;
use std::time::Instant;

use crate::app::{Cassie, CassieError};
use crate::config::{CassieRuntimeConfig, OperatorSwitchingEnabled};
use crate::runtime::QueryExecutionControls;

use super::{
    execute_loaded_join, reserve_join_rows, BatchRow, BinaryOp, Expr, JoinKind, JoinRowsSpec,
    SourceExecutionEnv, Value,
};

#[test]
fn should_record_spill_fallback_before_merge_retention_failure() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-loaded-spill-fallback-{}",
        uuid::Uuid::new_v4()
    ));
    let mut config = CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 1_023;
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 2;
    config.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone())
        .expect("loaded spill phase fixture");
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie: &cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    // These already loaded rows isolate operator selection from source qualification.
    let left = [BatchRow::new(vec![("a".to_owned(), Value::Int64(1))])];
    let right = [BatchRow::new(vec![("b".to_owned(), Value::Int64(2))])];
    let left_template = BatchRow::new(Vec::new());
    let right_template = BatchRow::new(Vec::new());
    let on = Expr::Binary {
        left: Box::new(Expr::Column("a".to_owned())),
        op: BinaryOp::Eq,
        right: Box::new(Expr::Column("b".to_owned())),
    };
    let input_memory = reserve_join_rows(&env, &left, &right)
        .expect("both complete operator inputs fit simultaneously");
    let input_bytes = controls.current_query_memory_bytes();
    assert!(input_bytes > 0);
    assert!(input_bytes < controls.query_memory_budget_bytes);
    assert_eq!(super::estimate_vectorized_join_bytes(1, 1), 1_024);
    let before = cassie.runtime.snapshot();

    // Act
    let error = match execute_loaded_join(
        &env,
        JoinRowsSpec {
            predicate_context: None,
            sources: None,
            kind: JoinKind::Inner,
            on: &on,
            left_rows: &left,
            right_rows: &right,
            left_template: &left_template,
            right_template: &right_template,
            row_budget: Some(1),
        },
        &["a".to_owned()],
        &["b".to_owned()],
    ) {
        Ok(_) => panic!("merge replacement must reject its retained keyed state"),
        Err(error) => CassieError::from(error),
    };

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    let after = cassie.runtime.snapshot();
    assert_eq!(
        after.joins.vectorized_fallbacks - before.joins.vectorized_fallbacks,
        1
    );
    assert_eq!(
        after.joins.vectorized_spill_fallbacks - before.joins.vectorized_spill_fallbacks,
        1
    );
    assert_eq!(
        after.joins.last_vectorized_fallback_reason,
        "spill_budget_exceeded"
    );
    assert_eq!(after.joins.vectorized_joins, before.joins.vectorized_joins);
    assert_eq!(after.joins.merge_joins, before.joins.merge_joins);
    assert_eq!(after.joins.executions, before.joins.executions);
    assert_eq!(
        after.joins.output_rows_total,
        before.joins.output_rows_total
    );
    assert_eq!(controls.current_query_memory_bytes(), input_bytes);
    eprintln!(
        "loaded spill probe: input_bytes={input_bytes}, vectorized_estimate=1024, budget=1023"
    );
    drop(left);
    drop(right);
    drop(input_memory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
