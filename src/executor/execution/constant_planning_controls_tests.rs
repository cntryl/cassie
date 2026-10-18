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
    let cases = [
        ("VALUES (CAST('bad' AS TEXT)::SMALLINT)", "22P02"),
        ("VALUES (CAST('bad' AS TEXT)::INT)", "22P02"),
        ("VALUES (CAST('bad' AS TEXT)::BIGINT)", "22P02"),
        ("VALUES (CAST('32768' AS TEXT)::SMALLINT)", "22003"),
        ("VALUES (CAST('2147483648' AS TEXT)::INT)", "22003"),
        (
            "VALUES (CAST('9223372036854775808' AS TEXT)::BIGINT)",
            "22003",
        ),
    ];
    for (sql, sqlstate) in cases {
        let plan = logical(sql);
        let controls = controls(1_048_576);
        // Act
        let result = check_plan(&plan, &controls).map_err(CassieError::from);
        // Assert
        assert_eq!(
            result.expect_err("invalid integer").descriptor().sql_state,
            sqlstate,
            "{sql}"
        );
        assert!(controls.peak_query_memory_bytes() > 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_skip_large_parameters_given_a_dead_bind_branch_when_the_budget_is_small() {
    // Arrange
    let plan = logical("SELECT CASE WHEN $1::BOOLEAN THEN $2::BYTEA::INT2 ELSE 7::INT2 END");
    let controls = controls(16384);
    let parameters = [
        crate::types::Value::Bool(false),
        crate::types::Value::String(format!("\\x{}", "00".repeat(100_000))),
    ];
    // Act
    let result = super::check_plan_with_parameters(&plan, &controls, &parameters);
    // Assert
    result.expect("dead parameter is neither copied nor charged");
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(controls.peak_query_memory_bytes() < 16384);
}

#[test]
fn should_admit_large_parameters_given_a_live_bind_branch_when_cloning_would_exceed_the_budget() {
    // Arrange
    let plan = logical("SELECT CASE WHEN $1::BOOLEAN THEN $2::BYTEA::INT2 ELSE 7::INT2 END");
    let controls = controls(16384);
    let parameters = [
        crate::types::Value::Bool(true),
        crate::types::Value::String(format!("\\x{}", "00".repeat(100_000))),
    ];
    // Act
    let result =
        super::check_plan_with_parameters(&plan, &controls, &parameters).map_err(CassieError::from);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_skip_large_parameters_given_deciding_boolean_inputs_when_bind_planning_short_circuits() {
    // Arrange
    let cases = [
        ("SELECT $1::BOOLEAN AND ($2::BYTEA::INT2 > 0)", false),
        ("SELECT $1::BOOLEAN OR ($2::BYTEA::INT2 > 0)", true),
    ];
    // Act
    let results = cases.map(|(sql, value)| {
        let plan = logical(sql);
        let controls = controls(16384);
        let parameters = [
            crate::types::Value::Bool(value),
            crate::types::Value::String(format!("\\x{}", "00".repeat(100_000))),
        ];
        let result = super::check_plan_with_parameters(&plan, &controls, &parameters);
        (
            result,
            controls.current_query_memory_bytes(),
            controls.peak_query_memory_bytes(),
        )
    });
    // Assert
    for (result, current, peak) in results {
        result.expect("deciding input skips the large parameter");
        assert_eq!(current, 0);
        assert!(peak < 16384);
    }
}

#[test]
fn should_deny_integer_bytea_constants_given_an_insufficient_budget_when_bind_checks_output_allocation(
) {
    // Arrange
    let plan = logical("SELECT 1::INT2::BYTEA");
    let controls = controls(1);
    // Act
    let result = check_plan(&plan, &controls).map_err(CassieError::from);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_expanding_bytea_input_given_a_small_budget_when_constant_output_is_allocated() {
    // Arrange
    let plan = logical(&format!(
        "SELECT CAST('{}' AS TEXT)::BYTEA",
        "a".repeat(100_000)
    ));
    let controls = controls(16384);
    // Act
    let result = check_plan(&plan, &controls).map_err(CassieError::from);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.peak_query_memory_bytes(), 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_bytea_input_scratch_given_an_invalid_escape_when_planning_rejects_the_input() {
    // Arrange
    let plan = logical("SELECT CAST('\\400' AS TEXT)::BYTEA");
    let controls = controls(16384);
    // Act
    let result = check_plan(&plan, &controls).map_err(CassieError::from);
    // Assert
    assert_eq!(
        result
            .expect_err("invalid octal escape")
            .descriptor()
            .sql_state,
        "22P02"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_skip_expanding_bytea_input_given_a_dead_branch_when_constant_memory_is_small() {
    // Arrange
    let plan = logical(&format!(
        "SELECT CASE WHEN FALSE THEN CAST('{}' AS TEXT)::BYTEA ELSE NULL::BYTEA END",
        "a".repeat(100_000)
    ));
    let controls = controls(16384);
    // Act
    let result = check_plan(&plan, &controls);
    // Assert
    result.expect("dead conversion allocates no output");
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
