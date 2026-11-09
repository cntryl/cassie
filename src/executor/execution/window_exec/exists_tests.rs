//! Reached aggregate arguments retain controls on the EXISTS window path.
use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::executor::execution::source::SourceExecutionEnv;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::{QueryStatement, SelectStatement};
use crate::types::Value;
use std::collections::HashMap;
use std::time::Instant;

fn select(filter: &str) -> SelectStatement {
    let sql = format!(
        "SELECT var_pop((doc ->> 'n')::numeric) FILTER (WHERE {filter}) OVER () FROM records"
    );
    let parsed = crate::sql::parse_statement(&sql).expect("window SQL");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    *select
}

fn run(
    cassie: &Cassie,
    select: &SelectStatement,
    controls: &QueryExecutionControls,
) -> Result<Vec<Batch>, QueryError> {
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls,
    };
    let row = super::super::BatchRow::new(vec![(
        "doc".into(),
        Value::Json(serde_json::json!({"n": "3.125"})),
    )]);
    apply_resolving(
        &env,
        &CteContext::new(),
        &select.source,
        vec![vec![row]],
        &select.projection,
        None,
    )
}

#[test]
fn should_cancel_during_reached_json_argument_scan_on_exists_window_path() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let select = select("EXISTS (SELECT 1)");
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    crate::executor::filter::cancel_raw_scan_once(Some(cancel.clone()));

    // Act
    let result = run(&cassie, &select, &controls);

    // Assert
    crate::executor::filter::cancel_raw_scan_once(None);
    assert!(
        cancel.is_cancelled(),
        "actual JSON scan must reach the hook"
    );
    assert!(result.is_err());
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_skip_the_argument_when_exists_window_filter_is_false_or_null() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    for predicate in ["FALSE AND EXISTS (SELECT 1)", "NULL AND EXISTS (SELECT 1)"] {
        let select = select(predicate);
        let cancel = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancel.clone(),
        );
        crate::executor::filter::cancel_raw_scan_once(Some(cancel.clone()));

        // Act
        let output = run(&cassie, &select, &controls).expect("filtered frame");

        // Assert
        crate::executor::filter::cancel_raw_scan_once(None);
        assert!(!cancel.is_cancelled(), "excluded argument was reached");
        assert!(matches!(
            output[0][0].entries().last().expect("result").1,
            Value::Null
        ));
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
