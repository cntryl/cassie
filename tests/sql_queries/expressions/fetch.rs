use crate::support_fetch_sql::{fixture, physical};
use cassie::app::CassieError;
use cassie::runtime::{QueryCancellationHandle, QueryExecutionControls};
use cassie::types::Value;

#[test]
fn should_fetch_filtered_stored_rows_given_ordered_bounds_when_offset_precedes_fetch() {
    // Arrange
    let fixture = fixture("fetch-stored-filter");
    // Act
    let result = fixture.execute(
        "SELECT v FROM fetch_rows WHERE v>7 ORDER BY v DESC OFFSET 1 ROW FETCH FIRST ROW ONLY",
    );
    // Assert
    assert_eq!(
        result.expect("stored FETCH").rows(),
        &[vec![Value::Int64(8)]]
    );
}

#[test]
fn should_rebind_cached_fetch_counts_given_repeated_executions_when_parameter_values_change() {
    // Arrange
    let fixture = fixture("fetch-cache");
    let sql = "SELECT v FROM fetch_rows ORDER BY v FETCH NEXT $1 ROWS ONLY OFFSET $2 ROWS";
    // Act
    let first = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(1), Value::Int64(0)],
    );
    let second = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(2), Value::Int64(1)],
    );
    // Assert
    assert_eq!(
        first.expect("first bounds").rows(),
        &[vec![Value::Int64(7)]]
    );
    assert_eq!(
        second.expect("new bounds").rows(),
        &[vec![Value::Int64(8)], vec![Value::Int64(9)]]
    );
}

#[test]
fn should_prune_unused_fetch_bounds_given_cte_definitions_when_only_the_root_query_runs() {
    // Arrange
    let fixture = fixture("fetch-unused-cte");
    // Act
    let result = fixture.execute("WITH unused AS (SELECT v FROM fetch_rows FETCH FIRST (-((-32768)::SMALLINT)) ROWS ONLY) SELECT v FROM fetch_rows ORDER BY v FETCH NEXT ROW ONLY");
    // Assert
    assert_eq!(
        result.expect("unused bound stays unevaluated").rows(),
        &[vec![Value::Int64(7)]]
    );
}

#[test]
fn should_resolve_fetch_before_deleting_rows_given_an_exists_count_when_the_bound_is_zero() {
    // Arrange
    let fixture = fixture("fetch-delete");
    let sql = "DELETE FROM fetch_rows WHERE EXISTS (SELECT 7 FETCH FIRST $1 ROWS ONLY)";
    // Act
    fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Int64(0)])
        .expect("zero bound");
    let after_zero = fixture.rows("SELECT v FROM fetch_rows ORDER BY v");
    fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Int64(1)])
        .expect("positive bound");
    // Assert
    assert_eq!(
        after_zero,
        vec![
            vec![Value::Int64(7)],
            vec![Value::Int64(8)],
            vec![Value::Int64(9)]
        ]
    );
    assert_eq!(
        fixture.rows("SELECT v FROM fetch_rows"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_resolve_fetch_before_updating_rows_given_an_exists_count_when_the_bound_is_zero() {
    // Arrange
    let fixture = fixture("fetch-update");
    let sql = "UPDATE fetch_rows SET v=v+1 WHERE EXISTS (SELECT 7 FETCH NEXT $1 ROWS ONLY)";
    // Act
    fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Int64(0)])
        .expect("zero bound");
    let after_zero = fixture.rows("SELECT v FROM fetch_rows ORDER BY v");
    fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Int64(1)])
        .expect("positive bound");
    // Assert
    assert_eq!(
        after_zero,
        vec![
            vec![Value::Int64(7)],
            vec![Value::Int64(8)],
            vec![Value::Int64(9)]
        ]
    );
    assert_eq!(
        fixture.rows("SELECT v FROM fetch_rows ORDER BY v"),
        vec![
            vec![Value::Int64(8)],
            vec![Value::Int64(9)],
            vec![Value::Int64(10)]
        ]
    );
}

#[test]
fn should_preserve_fetch_bounds_given_a_stored_view_when_its_rows_are_selected() {
    // Arrange
    let fixture = fixture("fetch-view");
    fixture.execute("CREATE VIEW fetch_view AS SELECT v FROM fetch_rows ORDER BY v FETCH FIRST (1+1) ROWS ONLY OFFSET 1 ROW").expect("view");
    // Act
    let result = fixture.execute("SELECT v FROM fetch_view ORDER BY v");
    // Assert
    assert_eq!(
        result.expect("view bounds").rows(),
        &[vec![Value::Int64(8)], vec![Value::Int64(9)]]
    );
}

#[test]
fn should_admit_fetch_plan_copies_given_a_small_memory_budget_when_direct_execution_resolves_parameters(
) {
    // Arrange
    let fixture = fixture("fetch-memory");
    let plan = physical(&fixture);
    let mut limits = cassie::config::CassieRuntimeConfig::default().limits;
    limits.query_memory_budget_bytes = 1;
    let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::Int64(0)],
        &controls,
    );
    // Assert
    assert!(result
        .expect_err("pre-copy admission")
        .to_string()
        .contains("query memory budget exceeded"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_fetch_given_a_cancelled_query_when_direct_execution_starts() {
    // Arrange
    let fixture = fixture("fetch-cancel");
    let plan = physical(&fixture);
    let limits = cassie::config::CassieRuntimeConfig::default().limits;
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls =
        QueryExecutionControls::with_cancellation(&limits, std::time::Instant::now(), cancellation);
    // Act
    let result = cassie::executor::run_with_controls(
        &fixture.cassie,
        &plan,
        vec![Value::Int64(0)],
        &controls,
    );
    // Assert
    assert!(matches!(
        result.map_err(CassieError::from),
        Err(CassieError::QueryCancelled)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
