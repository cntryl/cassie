//! Admission and cancellation for dependency analysis, before CTE execution.
use super::cte_reachability::{used, SelectParts};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::{QueryStatement, SelectStatement};
use std::time::Instant;

fn select(sql: &str) -> SelectStatement {
    let parsed = crate::sql::parse_statement(sql).expect("parse");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT")
    };
    *select
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
fn should_reject_dependency_scratch_given_small_budgets_when_ctes_are_present() {
    // Arrange
    let select = select("WITH a AS (SELECT 1) SELECT 7");
    let controls = controls(1);
    // Act
    let result = used(&SelectParts::of(&select), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
#[test]
fn should_own_dependency_masks_given_returned_analysis_when_execution_has_not_started() {
    // Arrange
    let select = select("WITH a AS (SELECT 1) SELECT 7");
    let controls = controls(1_048_576);
    // Act
    let live = used(&SelectParts::of(&select), &controls).expect("analysis");
    // Assert
    assert!(!live.contains(0));
    assert_eq!(controls.current_query_memory_bytes(), 1);
    assert!(controls.peak_query_memory_bytes() > 1);
    drop(live);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
#[test]
fn should_cancel_dependency_analysis_given_cancelled_controls_when_ctes_are_present() {
    // Arrange
    let select = select("WITH a AS (SELECT 1) SELECT 7");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation,
    );
    // Act
    let result = used(&SelectParts::of(&select), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}
#[test]
fn should_expire_dependency_analysis_given_elapsed_deadlines_when_ctes_are_present() {
    // Arrange
    let select = select("WITH a AS (SELECT 1) SELECT 7");
    let mut controls = controls(1_048_576);
    controls.deadline = Some(Instant::now());
    // Act
    let result = used(&SelectParts::of(&select), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::DeadlineExceeded)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
#[test]
fn should_release_dependency_scratch_given_excessive_depth_when_walks_are_bounded() {
    // Arrange
    let mut select = select("WITH a AS (SELECT 1) SELECT TRUE");
    let mut expr = crate::sql::ast::Expr::BoolLiteral(true);
    for _ in 0..129 {
        expr = crate::sql::ast::Expr::Not {
            expr: Box::new(expr),
        };
    }
    select.filter = Some(expr);
    let controls = controls(1_048_576);
    // Act
    let result = used(&SelectParts::of(&select), &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
#[test]
fn should_skip_dependency_scratch_given_no_ctes_when_budgets_are_small() {
    // Arrange
    let select = select("SELECT 7");
    let controls = controls(1);
    // Act
    let live = used(&SelectParts::of(&select), &controls).expect("no dependencies");
    // Assert
    drop(live);
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_reach_outer_definitions_given_shadowing_bodies_when_local_names_are_not_yet_visible() {
    // Arrange
    let select = select("WITH a AS (SELECT 7::INT AS n) SELECT n FROM (WITH a AS (SELECT n FROM a) SELECT n FROM a) q");
    let controls = controls(1_048_576);
    // Act
    let live = used(&SelectParts::of(&select), &controls).expect("dependencies");
    // Assert
    assert!(live.contains(0));
    drop(live);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_prune_outer_dependencies_given_unused_nested_definitions_when_their_bodies_are_unreachable(
) {
    // Arrange
    let select = select("WITH a AS (SELECT 1/0 AS n) SELECT n FROM (WITH unused AS (SELECT n FROM a) SELECT 7::INT AS n) q");
    let controls = controls(1_048_576);
    // Act
    let live = used(&SelectParts::of(&select), &controls).expect("dependencies");
    // Assert
    assert!(!live.contains(0));
}

#[test]
fn should_preserve_qualified_relations_given_matching_cte_names_when_schema_authority_differs() {
    // Arrange
    let select = select("WITH a AS (SELECT 1/0 AS n) SELECT n FROM public.a");
    let controls = controls(1_048_576);
    // Act
    let live = used(&SelectParts::of(&select), &controls).expect("dependencies");
    // Assert
    assert!(!live.contains(0));
}

#[test]
fn should_terminate_dependency_cycles_given_recursive_self_reads_when_the_cte_is_reached() {
    // Arrange
    let select = select(
        "WITH RECURSIVE a(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM a WHERE n<3) SELECT n FROM a",
    );
    let controls = controls(1_048_576);
    // Act
    let live = used(&SelectParts::of(&select), &controls).expect("dependencies");
    // Assert
    assert!(live.contains(0));
    drop(live);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
