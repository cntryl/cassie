//! Deferred tests of the selected physical SELECT row-directory handoff.
use super::*;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryCancellationHandle;

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
fn plan(cassie: &Cassie, suffix: &str) -> PhysicalPlan {
    let statement = crate::sql::parse_statement(&format!(
        "SELECT json_object_keys('{{\"a\":1,\"b\":2}}') AS keys{suffix}"
    ))
    .expect("SQL");
    let logical = build_logical_plan_in_session(cassie, None, &statement).expect("logical");
    crate::planner::physical::build(logical)
}
fn physical_rows(
    cassie: &Cassie,
    plan: &PhysicalPlan,
    controls: &QueryExecutionControls,
) -> Result<owned_rows::OwnedRows, QueryError> {
    execute_physical_plan(
        cassie,
        None,
        plan,
        &mut CteContext::new(),
        &HashMap::new(),
        &[],
        controls,
    )
}

#[test]
fn should_keep_selected_flat_directory_owned_through_independent_result_and_last_clone() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let controls = controls(32 * 1024 * 1024);
    let plan = plan(&cassie, "");
    let rows = physical_rows(&cassie, &plan, &controls).expect("rows");
    assert!(rows.directory_bytes() > 0, "actual physical selected route");
    let source_bytes = std::rc::Rc::new(std::cell::Cell::new(0));
    let overlap_bytes = std::rc::Rc::new(std::cell::Cell::new(0));
    owned_rows::observe_result_once(
        std::rc::Rc::clone(&source_bytes),
        std::rc::Rc::clone(&overlap_bytes),
    );
    // Act
    let result = build_select_result(&cassie, None, &plan, rows, &HashMap::new(), &controls)
        .expect("result");
    let last = result.clone();
    drop(result);
    // Assert
    assert_eq!(
        last.rows(),
        &[
            vec![Value::String("a".into())],
            vec![Value::String("b".into())]
        ]
    );
    assert!(source_bytes.get() > 0);
    assert!(overlap_bytes.get() > source_bytes.get());
    assert!(controls.current_query_memory_bytes() > 0);
    drop(last);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_selected_source_and_partial_result_on_actual_handoff_cancellation() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let cancel = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancel.clone(),
    );
    let plan = plan(&cassie, "");
    let rows = physical_rows(&cassie, &plan, &controls).expect("rows");
    assert!(rows.directory_bytes() > 0);
    owned_rows::cancel_result_once(cancel.clone());
    // Act
    let result = build_select_result(&cassie, None, &plan, rows, &HashMap::new(), &controls);
    // Assert
    assert!(cancel.is_cancelled());
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_leave_pagination_and_logical_execution_as_legacy_fallbacks() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let controls = controls(32 * 1024 * 1024);
    let paginated = plan(&cassie, " LIMIT 1");
    // Act
    let rows = physical_rows(&cassie, &paginated, &controls).expect("rows");
    // Assert
    assert_eq!(rows.directory_bytes(), 0);
    assert_eq!(rows.len(), 1);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_foreign_result_tracker_and_release_source_directory() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let source = controls(32 * 1024 * 1024);
    let foreign = controls(32 * 1024 * 1024);
    let plan = plan(&cassie, "");
    let rows = physical_rows(&cassie, &plan, &source).expect("rows");
    assert!(rows.directory_bytes() > 0);
    // Act
    let result = build_select_result(&cassie, None, &plan, rows, &HashMap::new(), &foreign);
    // Assert
    assert!(result.is_err());
    assert_eq!(source.current_query_memory_bytes(), 0);
    assert_eq!(foreign.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_measured_source_and_result_overlap_peak() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let generous = controls(32 * 1024 * 1024);
    let plan = plan(&cassie, "");
    let rows = physical_rows(&cassie, &plan, &generous).expect("rows");
    assert!(rows.directory_bytes() > 0);
    let result = build_select_result(&cassie, None, &plan, rows, &HashMap::new(), &generous)
        .expect("result");
    let peak = generous.peak_query_memory_bytes();
    drop(result);
    assert_eq!(generous.current_query_memory_bytes(), 0);
    // Act / Assert
    for (budget, succeeds) in [(peak, true), (peak - 1, false)] {
        let limited = controls(budget);
        let result = physical_rows(&cassie, &plan, &limited).and_then(|rows| {
            build_select_result(&cassie, None, &plan, rows, &HashMap::new(), &limited)
        });
        assert_eq!(result.is_ok(), succeeds);
        drop(result);
        assert_eq!(limited.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_distinct_selected_output_owned_and_leave_empty_or_set_outputs_legacy() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let controls = controls(32 * 1024 * 1024);
    for (sql, tracked, length) in [
        (
            r#"SELECT DISTINCT json_object_keys('{"a":1,"b":2}') AS keys"#,
            true,
            2,
        ),
        (r#"SELECT json_object_keys('{}') AS keys"#, false, 0),
        (
            r#"SELECT json_object_keys('{"a":1}') AS keys UNION SELECT 'b' AS keys"#,
            false,
            2,
        ),
    ] {
        let statement = crate::sql::parse_statement(sql).expect("SQL");
        let logical = build_logical_plan_in_session(&cassie, None, &statement).expect("logical");
        let plan = crate::planner::physical::build(logical);
        // Act
        let rows = physical_rows(&cassie, &plan, &controls).expect("rows");
        // Assert
        assert_eq!(rows.directory_bytes() > 0, tracked, "{sql}");
        assert_eq!(rows.len(), length, "{sql}");
        drop(rows);
        assert_eq!(controls.current_query_memory_bytes(), 0, "{sql}");
    }
}

#[test]
fn should_preserve_logical_callers_as_bare_legacy_rows() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fixture");
    let controls = controls(32 * 1024 * 1024);
    let plan = plan(&cassie, "");
    // Act
    let rows = execute_plan(
        &cassie,
        None,
        &plan.logical,
        &mut CteContext::new(),
        &HashMap::new(),
        &[],
        &controls,
    )
    .expect("logical rows");
    // Assert
    assert_eq!(rows.len(), 2);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
