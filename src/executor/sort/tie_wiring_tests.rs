use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use crate::runtime::QueryCancellationHandle;
use crate::types::{ArrayDimension, SqlArray};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::with_cancellation(
        &limits,
        Instant::now(),
        QueryCancellationHandle::new(),
    )
}

#[test]
fn should_build_extreme_bound_tie_with_owned_sort_key_at_measured_budget() {
    // Arrange
    let array = SqlArray::new(
        vec![ArrayDimension {
            length: 1,
            lower_bound: i32::MAX,
        }],
        vec![Value::Int64(7)],
    )
    .expect("valid array");
    let row = BatchRow::new(vec![("a".into(), Value::Array(array))]);
    let order = [OrderExpr {
        expr: Expr::IntegerLiteral(1),
        direction: SortDirection::Asc,
        nulls: None,
    }];
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &order,
        projection: &[],
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let baseline = controls(1_000_000);
    // Act
    let key = eval
        .row_key_with_context(
            &row,
            &order,
            &SortRetentionContext::default(),
            Some(&baseline),
        )
        .expect("sort key");
    let peak = baseline.peak_query_memory_bytes();
    // Assert
    assert_eq!(key.tie_key, "[2147483647:2147483647][7]");
    assert!(baseline.current_query_memory_bytes() >= key.tie_key.capacity());
    drop(key);
    assert_eq!(baseline.current_query_memory_bytes(), 0);
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let controls = controls(budget);
        let result = eval.row_key_with_context(
            &row,
            &order,
            &SortRetentionContext::default(),
            Some(&controls),
        );
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
