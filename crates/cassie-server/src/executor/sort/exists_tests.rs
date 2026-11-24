//! Resolver calls occur once per input key before any comparison.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::batch::BatchRow;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

#[test]
fn should_compute_deferred_sort_keys_once_before_comparisons() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let memory = Arc::new(controls.reserve_query_memory(512).expect("input marker"));
    let owner = Arc::downgrade(&memory);
    let rows = [3, 1, 2]
        .into_iter()
        .map(|id| {
            BatchRow::new(vec![("id".into(), Value::Int64(id))])
                .with_query_memory(Some(Arc::clone(&memory)))
        })
        .collect::<Vec<_>>();
    drop(memory);
    let statement =
        crate::sql::parse_statement("SELECT id FROM t ORDER BY EXISTS(SELECT 1 FROM u),id")
            .expect("sort keys");
    let crate::sql::ast::QueryStatement::Select(select) = statement.statement else {
        panic!("SELECT");
    };
    let functions = HashMap::new();
    let eval = EvalInput {
        order: &select.order,
        projection: &select.projection,
        params: &[],
        search_context: None,
        user_functions: &functions,
        session: None,
    };
    let calls = Cell::new(0);
    let evaluate = |row: &BatchRow, expr: &Expr| {
        calls.set(calls.get() + 1);
        let id = row.get("id").expect("id");
        Ok(if matches!(expr, Expr::Exists(_)) {
            Value::Bool(id == &Value::Int64(1))
        } else {
            id.clone()
        })
    };
    // Act
    let output =
        sort_batches_resolving(vec![rows], &eval, &controls, &evaluate).expect("deferred keys");
    // Assert
    assert_eq!(calls.get(), 6);
    assert_eq!(
        output
            .iter()
            .flatten()
            .map(|row| row.get("id").expect("id").clone())
            .collect::<Vec<_>>(),
        vec![Value::Int64(2), Value::Int64(3), Value::Int64(1)]
    );
    assert!(owner.upgrade().is_some());
    drop(output);
    assert!(owner.upgrade().is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
