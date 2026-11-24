//! The grouped kernel keeps live memory bounded by groups and honors controls.
use super::grouped::DISPATCH;
use super::*;
use crate::app::CassieError;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use std::time::Instant;

const GROUPS: usize = 8;
const SUMMARY: &str = "SELECT g, COUNT(*) AS c, COUNT(v) AS cv, SUM(v) AS s, MIN(v) AS lo, \
    MAX(v) AS hi, AVG(v) AS a FROM facts GROUP BY g";

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE facts (g INT, v BIGINT, f FLOAT)",
            vec![],
        )
        .expect("table");
    (cassie, session)
}

fn insert_rows(cassie: &Cassie, session: &CassieSession, rows: std::ops::Range<usize>) {
    let _ = session;
    cassie
        .midge
        .put_fresh_documents(
            "facts",
            rows.map(|row| {
                (
                    Some(format!("{row:08}")),
                    serde_json::json!({"g": row % GROUPS, "v": row, "f": row % 7}),
                )
            })
            .collect(),
        )
        .expect("rows");
}

fn plan(cassie: &Cassie, session: &CassieSession, sql: &str) -> LogicalPlan {
    let statement = crate::sql::parse_statement(sql).expect("statement");
    super::super::super::build_logical_plan_in_session(cassie, Some(session), &statement)
        .expect("plan")
}

fn controls(cassie: &Cassie) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now())
}

fn controls_with_budget(cassie: &Cassie, budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..cassie.runtime.limits()
        },
        Instant::now(),
    )
}

/// Runs the grouped kernel and returns its peak admitted bytes.
fn peak_for(cassie: &Cassie, session: &CassieSession, sql: &str) -> usize {
    let plan = plan(cassie, session, sql);
    let controls = controls(cassie);
    let rows = try_execute(cassie, Some(session), &plan, &[], &controls)
        .expect("grouped run")
        .expect("grouped kernel serves the query");
    assert_eq!(rows.len(), GROUPS);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    controls.peak_query_memory_bytes()
}

#[test]
fn should_keep_peak_query_memory_independent_of_input_rows_with_constant_groups() {
    // Arrange
    let (cassie, session) = fixture();
    let batch = crate::executor::batch::DEFAULT_BATCH_SIZE;
    insert_rows(&cassie, &session, 0..2 * batch);
    // Act
    let single = peak_for(&cassie, &session, SUMMARY);
    insert_rows(&cassie, &session, 2 * batch..8 * batch);
    let quadruple = peak_for(&cassie, &session, SUMMARY);
    // Assert
    assert!(single > 0);
    assert_eq!(quadruple, single);
}

#[test]
fn should_admit_the_grouped_query_at_exactly_its_peak_bytes() {
    // Arrange
    let (cassie, session) = fixture();
    insert_rows(
        &cassie,
        &session,
        0..3 * crate::executor::batch::DEFAULT_BATCH_SIZE,
    );
    let needed = peak_for(&cassie, &session, SUMMARY);
    let plan = plan(&cassie, &session, SUMMARY);
    let below = controls_with_budget(&cassie, needed - 1);
    let exact = controls_with_budget(&cassie, needed);
    // Act
    let rejected = try_execute(&cassie, Some(&session), &plan, &[], &below);
    let admitted = try_execute(&cassie, Some(&session), &plan, &[], &exact);
    // Assert
    assert!(matches!(
        rejected,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(below.current_query_memory_bytes(), 0);
    assert_eq!(admitted.expect("admitted").expect("served").len(), GROUPS);
}

#[test]
fn should_cancel_after_a_completed_batch_without_partial_output() {
    // Arrange
    let (cassie, session) = fixture();
    insert_rows(
        &cassie,
        &session,
        0..3 * crate::executor::batch::DEFAULT_BATCH_SIZE,
    );
    let plan = plan(&cassie, &session, SUMMARY);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime.limits(),
        Instant::now(),
        cancellation.clone(),
    );
    let _probe = workers::WaveCancellationProbe::arm(cancellation);
    let before = cassie.runtime.snapshot().parallel_aggregation;
    // Act
    let failure = try_execute(&cassie, Some(&session), &plan, &[], &controls)
        .expect_err("cancellation after the first batch");
    // Assert
    let after = cassie.runtime.snapshot().parallel_aggregation;
    assert!(failure.to_string().contains("cancel"));
    assert_eq!(after.fallback_aggregations, before.fallback_aggregations);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_see_an_extremum_that_only_exists_in_the_second_batch() {
    // Arrange
    let (cassie, session) = fixture();
    let batch = crate::executor::batch::DEFAULT_BATCH_SIZE;
    let late = batch + 76;
    cassie
        .midge
        .put_fresh_documents(
            "facts",
            (0..batch + 100)
                .map(|row| {
                    let v = match row {
                        late_row if late_row == late => serde_json::json!(1_000_000),
                        null_row if null_row == late + 1 => serde_json::Value::Null,
                        row => serde_json::json!(row),
                    };
                    (
                        Some(format!("{row:08}")),
                        serde_json::json!({"g": row % GROUPS, "v": v, "f": 1}),
                    )
                })
                .collect(),
        )
        .expect("rows");
    let sql = "SELECT g, MAX(v) AS hi, MIN(v) AS lo, COUNT(v) AS cv, COUNT(*) AS c \
               FROM facts GROUP BY g";
    // Act
    let typed = cassie.execute_sql(&session, sql, vec![]).expect("typed");
    let reason = cassie
        .runtime
        .snapshot()
        .parallel_aggregation
        .last_fallback_reason;
    let forced_row_path = cassie
        .execute_sql(&session, &format!("{sql} HAVING COUNT(*) > 0"), vec![])
        .expect("row path");
    // Assert
    let group = late % GROUPS;
    assert_eq!(reason, DISPATCH);
    assert_eq!(typed.rows(), forced_row_path.rows());
    assert_eq!(typed.rows()[group][1], Value::Int64(1_000_000));
    assert_eq!(
        typed.rows()[group][0],
        Value::Int64(i64::try_from(group).expect("group"))
    );
}

#[test]
fn should_return_no_groups_for_empty_inputs() {
    // Arrange
    let (cassie, session) = fixture();
    // Act
    let empty = cassie
        .execute_sql(
            &session,
            "SELECT g, COUNT(*) AS c FROM facts GROUP BY g",
            vec![],
        )
        .expect("empty table");
    insert_rows(&cassie, &session, 0..10);
    let selected_none = cassie
        .execute_sql(
            &session,
            "SELECT g, COUNT(*) AS c FROM facts WHERE v > 1000 GROUP BY g",
            vec![],
        )
        .expect("empty selection");
    // Assert
    assert_eq!(empty.rows().len(), 0);
    assert_eq!(selected_none.rows().len(), 0);
    assert_eq!(
        cassie
            .runtime
            .snapshot()
            .parallel_aggregation
            .last_fallback_reason,
        DISPATCH
    );
}

#[test]
fn should_keep_prepared_parameters_on_the_typed_path() {
    // Arrange
    let (cassie, session) = fixture();
    insert_rows(&cassie, &session, 0..64);
    let sql = "SELECT g, SUM(v) AS s FROM facts WHERE v > $1 GROUP BY g";
    // Act
    let typed = cassie
        .execute_sql(&session, sql, vec![Value::Int64(40)])
        .expect("typed");
    let reason = cassie
        .runtime
        .snapshot()
        .parallel_aggregation
        .last_fallback_reason;
    let forced_row_path = cassie
        .execute_sql(
            &session,
            &format!("{sql} HAVING COUNT(*) > 0"),
            vec![Value::Int64(40)],
        )
        .expect("row path");
    // Assert
    assert_eq!(reason, DISPATCH);
    assert_eq!(typed.rows(), forced_row_path.rows());
    assert_eq!(typed.rows().len(), GROUPS);
}

#[test]
fn should_report_aggregate_overflow_like_the_row_path() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .midge
        .put_fresh_documents(
            "facts",
            vec![
                (
                    Some("a".into()),
                    serde_json::json!({"g": 1, "v": i64::MAX, "f": 1.0e308}),
                ),
                (
                    Some("b".into()),
                    serde_json::json!({"g": 1, "v": i64::MAX, "f": 1.0e308}),
                ),
                (
                    Some("c".into()),
                    serde_json::json!({"g": 2, "v": 1, "f": 1.5}),
                ),
            ],
        )
        .expect("rows");
    for sql in [
        "SELECT g, SUM(v) AS s, AVG(v) AS a FROM facts GROUP BY g",
        "SELECT g, SUM(f) AS s FROM facts GROUP BY g",
        "SELECT g, AVG(f) AS a FROM facts GROUP BY g",
    ] {
        // Act
        let typed = cassie
            .execute_sql(&session, sql, vec![])
            .map(|result| result.rows().to_vec())
            .map_err(|error| error.to_string());
        let row = cassie
            .execute_sql(&session, &format!("{sql} HAVING COUNT(*) > 0"), vec![])
            .map(|result| result.rows().to_vec())
            .map_err(|error| error.to_string());
        // Assert
        assert!(typed == row, "{sql}");
    }
    let overflow = cassie.execute_sql(
        &session,
        "SELECT g, SUM(f) AS s FROM facts GROUP BY g",
        vec![],
    );
    assert!(overflow.is_err());
}
