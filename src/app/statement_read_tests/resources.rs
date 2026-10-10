//! Measured owner charge and materialized-source overlap for portal calibration.
use super::{direct_plan, execute, Cassie, CassieSession, Value};
use crate::executor::{QueryError, QueryResult};
use crate::planner::physical::PhysicalPlan;
use crate::runtime::QueryExecutionControls;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

struct SourceRun {
    rows: Result<Vec<Vec<Value>>, QueryError>,
    peak: usize,
    owner_charge: usize,
}

fn source_fixture(table: &str, payload_bytes: usize) -> (Cassie, CassieSession, PathBuf) {
    let path = std::env::temp_dir().join(format!("cassie-{table}-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        &format!("CREATE TABLE {table} (payload TEXT)"),
    );
    for index in 0..64 {
        cassie
            .midge
            .put_document(
                table,
                Some(format!("doc-{index:04}")),
                serde_json::json!({"payload":format!("{index:04}-{}", "x".repeat(payload_bytes))}),
            )
            .expect("seed exact source fixture");
    }
    execute(&cassie, &session, "BEGIN");
    (cassie, session, path)
}

fn controls_for_budget(cassie: &Cassie, budget: usize) -> QueryExecutionControls {
    let mut limits = cassie.runtime.limits();
    limits.query_memory_budget_bytes = budget;
    QueryExecutionControls::from_limits(&limits, Instant::now())
}

fn retire_result(result: Result<QueryResult, QueryError>) -> Result<Vec<Vec<Value>>, QueryError> {
    match result {
        Ok(result) => {
            let rows = result.rows.clone();
            drop(result);
            Ok(rows)
        }
        Err(error) => Err(error),
    }
}

fn supplied_source_run(
    cassie: &Cassie,
    session: &CassieSession,
    plan: &Arc<PhysicalPlan>,
    budget: usize,
    case: &str,
) -> SourceRun {
    let controls = controls_for_budget(cassie, budget);
    let owner = session
        .capture_statement_read(&cassie.midge, &cassie.default_database, &controls)
        .expect("caller-supplied owner capture");
    let owner_charge = controls.current_query_memory_bytes();
    let captured = controls.with_statement_read(Some(Arc::clone(&owner)));
    let references_before = Arc::strong_count(&owner);
    let result =
        crate::executor::run_with_session_controls(cassie, Some(session), plan, vec![], &captured);
    let peak = controls.peak_query_memory_bytes();
    let references_after = Arc::strong_count(&owner);
    let rows = retire_result(result);
    let retained = controls.current_query_memory_bytes();
    drop(captured);
    drop(owner);
    let released = controls.current_query_memory_bytes();
    let workers = cassie.runtime.snapshot().runtime.active_operator_workers;
    println!(
        "supplied {case}: budget={budget} owner={owner_charge} peak={peak} refs={references_before}/{references_after} retained={retained} final={released} workers={workers}"
    );
    assert_eq!(references_before, 2);
    assert_eq!(references_after, references_before);
    assert_eq!(
        retained, owner_charge,
        "caller-owned charge survives {case}"
    );
    assert_eq!(released, 0);
    assert_eq!(workers, 0);
    assert!(peak <= budget);
    SourceRun {
        rows,
        peak,
        owner_charge,
    }
}

fn automatic_source_run(
    cassie: &Cassie,
    session: &CassieSession,
    plan: &Arc<PhysicalPlan>,
    budget: usize,
    case: &str,
) -> SourceRun {
    let controls = controls_for_budget(cassie, budget);
    let result =
        crate::executor::run_with_session_controls(cassie, Some(session), plan, vec![], &controls);
    let peak = controls.peak_query_memory_bytes();
    let rows = retire_result(result);
    let released = controls.current_query_memory_bytes();
    let workers = cassie.runtime.snapshot().runtime.active_operator_workers;
    println!("automatic {case}: budget={budget} peak={peak} final={released} workers={workers}");
    assert_eq!(released, 0, "automatic owner retires after {case}");
    assert_eq!(workers, 0);
    assert!(peak <= budget);
    SourceRun {
        rows,
        peak,
        owner_charge: 0,
    }
}

fn assert_exact_payloads(run: &SourceRun, payload_bytes: usize) {
    let rows = run.rows.as_ref().expect("admitted materialized source");
    assert_eq!(rows.len(), 64);
    let mut actual = rows
        .iter()
        .map(|row| match row.as_slice() {
            [Value::String(payload)] => payload.clone(),
            other => panic!("one projected text value: {other:?}"),
        })
        .collect::<Vec<_>>();
    actual.sort_unstable();
    let expected = (0..64)
        .map(|index| format!("{index:04}-{}", "x".repeat(payload_bytes)))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected, "exact projected payload multiset");
}

fn assert_memory_denial(run: &SourceRun) {
    let error = run
        .rows
        .as_ref()
        .expect_err("peak-minus-one denies overlap");
    assert!(matches!(
        error,
        QueryError::Cassie(crate::app::CassieError::ResourceLimit(_))
    ));
    assert!(error.to_string().contains("memory budget exceeded"));
}

#[test]
fn should_measure_combined_statement_owner_charge_during_materialized_source_execution() {
    // Arrange
    let (cassie, session, path) = source_fixture("portal_shared_memory", 1024);
    let plan = direct_plan(&cassie, "SELECT lower(payload) AS payload FROM portal_shared_memory WHERE payload IS NOT NULL LIMIT 1001 OFFSET 0");
    let calibration_budget = cassie.runtime.limits().query_memory_budget_bytes;
    let data_controls = controls_for_budget(&cassie, calibration_budget);
    let data = crate::midge::adapter::StatementDataRead::capture(
        &cassie.midge,
        &cassie.default_database,
        &data_controls,
    )
    .expect("Data-only capture");
    let data_charge = data_controls.current_query_memory_bytes();
    drop(data);
    let data_released = data_controls.current_query_memory_bytes();
    println!(
        "data-only: budget={calibration_budget} Transaction={} StatementDataRead={} charge={data_charge} final={data_released}",
        std::mem::size_of::<cntryl_midge::Transaction>(),
        std::mem::size_of::<crate::midge::adapter::StatementDataRead>()
    );

    // Act
    let measured = supplied_source_run(&cassie, &session, &plan, calibration_budget, "measure");
    let peak = measured.peak;
    let below_peak = peak.checked_sub(1).expect("positive measured peak");
    let exact = supplied_source_run(&cassie, &session, &plan, peak, "exact");
    let denied = supplied_source_run(&cassie, &session, &plan, below_peak, "peak-minus-one");
    let retry = supplied_source_run(&cassie, &session, &plan, peak, "fresh-retry");

    // Assert
    assert!(data_charge > 0);
    assert_eq!(data_released, 0);
    assert!(measured.owner_charge > data_charge);
    assert!(peak > measured.owner_charge);
    assert!(peak < calibration_budget);
    for admitted in [&measured, &exact, &retry] {
        assert_exact_payloads(admitted, 1024);
        assert_eq!(admitted.peak, peak);
        assert_eq!(admitted.owner_charge, measured.owner_charge);
    }
    assert_memory_denial(&denied);
    assert_eq!(denied.owner_charge, measured.owner_charge);
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

#[test]
fn should_release_automatically_captured_owners_after_materialized_source_denial() {
    // Arrange
    let (cassie, session, path) = source_fixture("denied_source", 1025);
    let plan = direct_plan(&cassie, "SELECT lower(payload) AS payload FROM denied_source WHERE payload IS NOT NULL LIMIT 1001 OFFSET 0");
    let calibration_budget = cassie.runtime.limits().query_memory_budget_bytes;

    // Act
    let measured = automatic_source_run(&cassie, &session, &plan, calibration_budget, "measure");
    let peak = measured.peak;
    let below_peak = peak.checked_sub(1).expect("positive measured peak");
    let exact = automatic_source_run(&cassie, &session, &plan, peak, "exact");
    let denied = automatic_source_run(&cassie, &session, &plan, below_peak, "peak-minus-one");
    let retry = automatic_source_run(&cassie, &session, &plan, peak, "fresh-retry");

    // Assert
    assert!(peak > 0);
    assert!(peak < calibration_budget);
    for admitted in [&measured, &exact, &retry] {
        assert_exact_payloads(admitted, 1025);
        assert_eq!(admitted.peak, peak);
    }
    assert_memory_denial(&denied);
    drop(session);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}
