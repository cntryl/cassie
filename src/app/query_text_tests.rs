//! Execution-local SQL text, admission and result ownership through the public query path.
use super::query_test_fixture::{controls, Fixture};
use super::{CassieError, ExecutionMode, QueryExecutionControls};
use crate::config::CassieRuntimeLimits;
use crate::types::Value;
use std::time::Instant;

#[test]
fn should_isolate_statement_text_given_concurrent_cloned_sessions_when_queries_execute() {
    // Arrange
    let fixture = Fixture::new();
    let barrier = std::sync::Barrier::new(4);
    // Act
    let rows = std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|index| {
                let barrier = &barrier;
                let cassie = fixture.cassie();
                let session = fixture.session.clone();
                scope.spawn(move || {
                    let sql = format!("/* concurrent {index} */ SELECT current_query() AS output");
                    barrier.wait();
                    for _ in 0..8 {
                        let result = cassie
                            .execute_sql(&session, &sql, vec![])
                            .expect("concurrent query");
                        assert_eq!(result.rows(), vec![vec![Value::String(sql.clone())]]);
                    }
                    session.statement_text().map(str::to_owned)
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("worker"))
            .collect::<Vec<_>>()
    });
    // Assert
    assert_eq!(rows, vec![None; 4]);
    assert_eq!(fixture.session.statement_text(), None);
}

#[test]
fn should_deny_statement_text_given_insufficient_memory_when_query_enters_execution() {
    // Arrange
    let fixture = Fixture::new();
    let sql = format!("/* {} */ SELECT current_query()", "x".repeat(4096));
    let controls = controls(1024);
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        &sql,
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(fixture.session.statement_text(), None);
}

#[test]
fn should_release_statement_context_given_cloned_results_when_the_final_owner_drops() {
    // Arrange
    let fixture = Fixture::new();
    let controls = controls(8 * 1024 * 1024);
    let sql = "/* retained */ SELECT current_query() AS output";
    // Act
    let result = fixture
        .cassie()
        .execute_sql_with_controls(
            &fixture.session,
            sql,
            vec![],
            ExecutionMode::SimpleQuery,
            &controls,
        )
        .expect("query");
    let retained = result.clone();
    drop(result);
    // Assert
    assert_eq!(retained.rows(), vec![vec![Value::String(sql.to_owned())]]);
    assert!(controls.current_query_memory_bytes() > 0);
    assert_eq!(fixture.session.statement_text(), None);
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_statement_text_given_an_execution_error_when_query_unwinds() {
    // Arrange
    let fixture = Fixture::new();
    let controls = controls(8 * 1024 * 1024);
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        "SELECT current_query(), 1 / 0",
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(fixture.session.statement_text(), None);
}

#[test]
fn should_leave_no_statement_text_given_precancellation_when_query_is_rejected() {
    // Arrange
    let fixture = Fixture::new();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    cancellation.cancel();
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        "SELECT current_query()",
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(fixture.session.statement_text(), None);
}

#[test]
fn should_admit_statement_text_scratch_given_large_comments_when_only_a_scalar_length_is_returned()
{
    // Arrange
    let fixture = Fixture::new();
    let controls = controls(12 * 1024);
    let sql = format!("/* {} */ SELECT length(current_query())", "x".repeat(4096));
    // Act
    let result = fixture.cassie().execute_sql_with_controls(
        &fixture.session,
        &sql,
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(fixture.session.statement_text(), None);
}

#[test]
fn should_preserve_explain_entry_text_given_analyzed_predicate_when_public_explain_executes() {
    // Arrange
    let fixture = Fixture::new();
    let controls = controls(8 * 1024 * 1024);
    // Act
    let result = fixture
        .cassie()
        .explain_sql_core(
            &fixture.session,
            "EXPLAIN ANALYZE SELECT 1 WHERE current_query() IS NULL",
            vec![],
            &controls,
        )
        .expect("explain");
    // Assert
    let rows = result.result().rows();
    assert!(
        matches!(&rows[0][0], Value::String(plan) if plan.contains("actual_rows=0")),
        "{rows:?}"
    );
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(fixture.session.statement_text(), None);
}
