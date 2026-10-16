//! Query controls remain active when constant checks run before portal creation.
use super::check_plan;
use crate::app::CassieError;
use crate::catalog::Catalog;
use crate::config::CassieRuntimeLimits;
use crate::planner::logical::LogicalPlan;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn logical(sql: &str) -> LogicalPlan {
    let parsed = crate::sql::parse_statement(sql).expect("parse");
    let bound = crate::sql::bind(parsed, &Catalog::new()).expect("bind");
    crate::planner::logical::plan(&bound).expect("logical plan")
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
fn should_cancel_constant_planning_given_cancelled_controls_when_no_portal_exists() {
    // Arrange
    let plan = logical("SELECT 1/0");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    // Act
    let result = check_plan(&plan, &controls).map_err(CassieError::from);
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_expire_constant_planning_given_elapsed_deadlines_when_no_portal_exists() {
    // Arrange
    let plan = logical("SELECT 1/0");
    let mut controls = controls(1_048_576);
    controls.deadline = Some(Instant::now());
    // Act
    let result = check_plan(&plan, &controls).map_err(CassieError::from);
    // Assert
    assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_values_scratch_given_small_budgets_when_bind_checks_constant_inputs() {
    // Arrange
    let plan = logical("VALUES ('wide text'::TEXT)");
    let controls = controls(64);
    // Act
    let result = check_plan(&plan, &controls).map_err(CassieError::from);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_values_scratch_given_folding_errors_when_constant_admission_is_owned() {
    // Arrange
    let plan = logical("VALUES (CAST('bad' AS TEXT)::INT)");
    let controls = controls(1_048_576);
    // Act
    let result = check_plan(&plan, &controls).map_err(CassieError::from);
    // Assert
    assert_eq!(
        result.expect_err("invalid integer").descriptor().sql_state,
        "22P02"
    );
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
