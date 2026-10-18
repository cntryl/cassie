//! CTE expansion cannot publish a partial tree or escape query controls.
use super::inline;
use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn logical(sql: &str) -> crate::planner::LogicalPlan {
    let parsed = crate::sql::parse_statement(sql).expect("parse");
    let bound = crate::sql::bind(parsed, &Catalog::new()).expect("bind");
    crate::planner::logical::plan(&bound).expect("plan")
}
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

#[test]
fn should_reject_cte_expansion_given_small_budget_when_tree_would_grow() {
    // Arrange
    let sql = format!("WITH chosen AS NOT MATERIALIZED (SELECT '{}' AS value) SELECT value||value||value FROM chosen", "x".repeat(4096));
    let mut plan = logical(&sql);
    let before = serde_json::to_value(&plan).unwrap();
    let controls = controls(12 * 1024);
    // Act
    let result = inline(&mut plan, &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(serde_json::to_value(plan).unwrap(), before);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_cte_expansion_given_cancelled_controls_when_no_tree_is_published() {
    // Arrange
    let mut plan =
        logical("WITH chosen AS NOT MATERIALIZED (SELECT 7 AS value) SELECT value FROM chosen");
    let before = serde_json::to_value(&plan).unwrap();
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    // Act
    let result = inline(&mut plan, &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(serde_json::to_value(plan).unwrap(), before);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_cte_expansion_given_deep_dependency_chain_when_substituting_expressions() {
    // Arrange
    let mut definitions = vec!["c0 AS NOT MATERIALIZED (SELECT 1 AS value)".to_owned()];
    for index in 1..160 {
        definitions.push(format!(
            "c{index} AS NOT MATERIALIZED (SELECT CAST(value AS INTEGER) AS value FROM c{})",
            index - 1
        ));
    }
    let mut plan = logical(&format!(
        "WITH {} SELECT value FROM c159",
        definitions.join(",")
    ));
    let before = serde_json::to_value(&plan).unwrap();
    let controls = controls(64 * 1024 * 1024);
    // Act
    let result = inline(&mut plan, &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(serde_json::to_value(plan).unwrap(), before);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_unpublished_cte_copy_given_growth_denial_when_clone_was_admitted() {
    // Arrange
    let columns = std::iter::repeat_n("value", 16)
        .collect::<Vec<_>>()
        .join("||");
    let mut plan = logical(&format!(
        "WITH chosen AS NOT MATERIALIZED (SELECT '{}' AS value) SELECT {columns} FROM chosen",
        "x".repeat(4096)
    ));
    let before = serde_json::to_value(&plan).unwrap();
    let measuring = controls(1_048_576);
    let allowance = super::memory::plan(&plan, &measuring).unwrap() * 4 + 16_384;
    let controls = controls(allowance);
    // Act
    let result = inline(&mut plan, &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(serde_json::to_value(plan).unwrap(), before);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_expire_cte_expansion_given_elapsed_deadline_when_no_tree_is_published() {
    // Arrange
    let mut plan =
        logical("WITH chosen AS NOT MATERIALIZED (SELECT 7 AS value) SELECT value FROM chosen");
    let before = serde_json::to_value(&plan).unwrap();
    let mut controls = controls(1_048_576);
    controls.deadline = Some(Instant::now());
    // Act
    let result = inline(&mut plan, &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
    assert_eq!(serde_json::to_value(plan).unwrap(), before);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_dependent_scope_copies_given_wide_producer_when_controls_bound_growth() {
    // Arrange
    let mut definitions = vec![format!(
        "c0 AS NOT MATERIALIZED (SELECT '{}' AS value)",
        "x".repeat(4096)
    )];
    for index in 1..12 {
        definitions.push(format!(
            "c{index} AS NOT MATERIALIZED (SELECT value||'x' AS value FROM c{})",
            index - 1
        ));
    }
    let original = logical(&format!(
        "WITH {} SELECT value FROM c11",
        definitions.join(",")
    ));
    let before = serde_json::to_value(&original).unwrap();
    let measuring = controls(64 * 1024 * 1024);
    let allowance = super::memory::plan(&original, &measuring).unwrap() * 4 + 16_384;
    let denied_controls = controls(allowance);
    let mut denied_plan = original.clone();
    let mut admitted_plan = original;
    // Act
    let denied = inline(&mut denied_plan, &Catalog::new(), &denied_controls);
    let admitted = inline(&mut admitted_plan, &Catalog::new(), &measuring)
        .expect("wide dependent scopes fit generous budget")
        .expect("expanded plan lease");
    // Assert
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    assert_eq!(serde_json::to_value(denied_plan).unwrap(), before);
    assert_eq!(denied_controls.current_query_memory_bytes(), 0);
    assert!(denied_controls.peak_query_memory_bytes() <= allowance);
    assert_ne!(serde_json::to_value(admitted_plan).unwrap(), before);
    assert!(measuring.current_query_memory_bytes() > 0);
    drop(admitted);
    assert_eq!(measuring.current_query_memory_bytes(), 0);
}
