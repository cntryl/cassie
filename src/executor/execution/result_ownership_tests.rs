use std::time::Instant;

use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::{ColumnMeta, QueryResult};

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn result(controls: &QueryExecutionControls) -> QueryResult {
    let _ = controls;
    QueryResult {
        columns: vec![ColumnMeta::text("variable_width")],
        rows: vec![vec![Value::String("retained result".repeat(64))]],
        command: "SELECT".to_owned(),
    }
}

#[test]
fn should_keep_result_memory_reserved_until_the_last_shared_owner_drops() {
    // Arrange
    let controls = controls(1_048_576);
    let result = result(&controls);

    // Act
    let retained = controls.current_query_memory_bytes();
    let alias = result.clone();
    drop(result);

    // Assert
    assert!(retained > 896, "the variable-width payload must remain admitted");
    assert_eq!(controls.current_query_memory_bytes(), retained);
    drop(alias);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_share_result_storage_without_allocating_a_deep_clone() {
    // Arrange
    let controls = controls(1_048_576);
    let result = result(&controls);
    let peak = controls.peak_query_memory_bytes();

    // Act
    let alias = result.clone();

    // Assert
    assert_eq!(result.rows.as_ptr(), alias.rows.as_ptr());
    assert_eq!(controls.peak_query_memory_bytes(), peak);
}
