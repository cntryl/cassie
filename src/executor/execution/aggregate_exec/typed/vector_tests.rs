//! Column-table aggregates over the vector path: memory is flat in the input,
//! and parallel waves honor the worker budget, cancellation and the snapshot.
use super::*;
use crate::app::CassieError;
use crate::config::CassieRuntimeConfig;
use crate::executor::scan::{Partitioning, TypedScanStream};
use std::time::Instant;

const GROUPS: usize = 8;
const SUMMARY: &str = "SELECT g, COUNT(*) AS c, SUM(v) AS s, MIN(v) AS lo, MAX(v) AS hi \
    FROM facts GROUP BY g";
const TOTAL: &str = "SELECT COUNT(*) AS c, SUM(v) AS s FROM facts";

fn fixture(workers: usize) -> (Cassie, CassieSession) {
    let mut config = CassieRuntimeConfig::default();
    config.limits.parallel_aggregation_workers = workers;
    config.limits.parallel_scan_workers = workers;
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE facts (g INT, v BIGINT) WITH (storage = column_store)",
            vec![],
        )
        .expect("table");
    (cassie, session)
}

fn insert_rows(cassie: &Cassie, rows: std::ops::Range<usize>) {
    cassie
        .midge
        .put_documents(
            "facts",
            rows.map(|row| {
                (
                    Some(format!("{row:08}")),
                    serde_json::json!({"g": row % GROUPS, "v": row}),
                )
            })
            .collect(),
        )
        .expect("rows");
}

fn segment(cassie: &Cassie) {
    cassie
        .midge
        .rebuild_column_store_summaries_for_diagnostics("facts")
        .expect("summaries");
}

fn plan(cassie: &Cassie, session: &CassieSession, sql: &str) -> LogicalPlan {
    let statement = crate::sql::parse_statement(sql).expect("statement");
    super::super::super::build_logical_plan_in_session(cassie, Some(session), &statement)
        .expect("plan")
}

fn controls(cassie: &Cassie, budget: Option<usize>) -> QueryExecutionControls {
    let mut limits = cassie.runtime.limits();
    if let Some(budget) = budget {
        limits.query_memory_budget_bytes = budget;
    }
    QueryExecutionControls::from_limits(&limits, Instant::now())
}

fn served(
    cassie: &Cassie,
    session: &CassieSession,
    sql: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    try_execute(
        cassie,
        Some(session),
        &plan(cassie, session, sql),
        &[],
        controls,
    )
}

/// Runs the query on the vector path and returns its peak admitted bytes.
fn peak_for(cassie: &Cassie, session: &CassieSession, sql: &str) -> usize {
    let controls = controls(cassie, None);
    let before = cassie.midge.column_vector_reads_for_diagnostics();
    let rows = served(cassie, session, sql, &controls)
        .expect("run")
        .expect("typed kernel serves the query");
    let delta = cassie
        .midge
        .column_vector_reads_for_diagnostics()
        .since(before);
    assert!(delta.batches > 0);
    assert_eq!(delta.row_documents_built, 0);
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    controls.peak_query_memory_bytes()
}

fn peak_at_one_and_four_times(workers: usize, segmented: bool) -> (usize, usize) {
    let (cassie, session) = fixture(workers);
    let batch = crate::executor::batch::DEFAULT_BATCH_SIZE;
    insert_rows(&cassie, 0..2 * batch);
    if segmented {
        segment(&cassie);
    }
    let single = peak_for(&cassie, &session, SUMMARY);
    insert_rows(&cassie, 2 * batch..8 * batch);
    if segmented {
        segment(&cassie);
    }
    (single, peak_for(&cassie, &session, SUMMARY))
}

#[test]
fn should_keep_grouped_peak_memory_equal_across_a_fourfold_row_growth_on_a_column_table() {
    // Arrange
    let workers = 1;

    // Act
    let (single, quadruple) = peak_at_one_and_four_times(workers, false);

    // Assert
    assert!(single > 0);
    assert_eq!(quadruple, single);
}

#[test]
fn should_keep_grouped_peak_memory_independent_of_the_rows_with_four_workers() {
    // Arrange
    let workers = 4;

    // Act
    let (single, quadruple) = peak_at_one_and_four_times(workers, true);

    // Assert
    // Which workers hold a batch at the same moment varies by a batch or so;
    // the peak must not follow the sixfold row growth.
    let batch = 2 * crate::executor::batch::DEFAULT_BATCH_SIZE * size_of::<Value>();
    assert!(single > 0);
    assert!(
        quadruple.abs_diff(single) <= 2 * batch,
        "{quadruple} vs {single}"
    );
}

#[test]
fn should_cancel_between_waves_without_partial_output_or_leaked_workers() {
    // Arrange
    let (cassie, session) = fixture(2);
    insert_rows(&cassie, 0..2_000);
    segment(&cassie);
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime.limits(),
        Instant::now(),
        cancellation.clone(),
    );
    let _probe = workers::WaveCancellationProbe::arm(cancellation);
    let before = cassie.runtime.snapshot().parallel_aggregation;

    // Act
    let failure = served(&cassie, &session, TOTAL, &controls);
    let after = cassie.runtime.snapshot();

    // Assert
    assert!(matches!(
        failure,
        Err(QueryError::Cassie(CassieError::QueryCanceled))
    ));
    assert_eq!(after.runtime.active_operator_workers, 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(after.parallel_aggregation.aggregations, before.aggregations);
}

#[test]
fn should_read_serially_with_the_budget_label_when_no_worker_permit_is_available() {
    // Arrange
    let (cassie, session) = fixture(4);
    insert_rows(&cassie, 0..2_000);
    segment(&cassie);
    let serial = served(&cassie, &session, TOTAL, &controls(&cassie, None))
        .expect("parallel run")
        .expect("served");
    let held = cassie
        .runtime
        .try_acquire_operator_workers(4)
        .expect("all permits");
    let before = cassie.midge.column_vector_reads_for_diagnostics();

    // Act
    let starved = served(&cassie, &session, TOTAL, &controls(&cassie, None))
        .expect("starved run")
        .expect("served");
    let delta = cassie
        .midge
        .column_vector_reads_for_diagnostics()
        .since(before);
    let reason = cassie
        .runtime
        .snapshot()
        .parallel_aggregation
        .last_fallback_reason;
    drop(held);

    // Assert
    assert_eq!(starved[0].entries(), serial[0].entries());
    assert_eq!(delta.parallel_scans, 0);
    assert_eq!(delta.last_serial_label, "column-vector-worker-budget");
    assert_eq!(reason, "column-vector-worker-budget");
    assert_eq!(cassie.runtime.snapshot().runtime.active_operator_workers, 0);
}

fn wave_run(
    cassie: &Cassie,
    session: &CassieSession,
    controls: &QueryExecutionControls,
) -> Result<usize, QueryError> {
    let mut stream =
        TypedScanStream::open(cassie, Some(session), "facts", &["v".to_owned()], controls)?
            .expect("vector source");
    let Some(Partitioning::Parallel(scan)) = stream.partitioning(4)? else {
        panic!("a segmented table runs in waves");
    };
    let mut rows = 0;
    scan.run(
        0,
        |batches| {
            let mut counted = 0_usize;
            while let Some(batch) = batches.next()? {
                counted += batch.len();
            }
            Ok(counted)
        },
        |counted| {
            rows += counted;
            Ok(())
        },
    )?;
    Ok(rows)
}

/// The bytes the stream and the wave hold before a worker reads anything.
fn wave_base_and_reservation(cassie: &Cassie, session: &CassieSession) -> (usize, usize) {
    let probe = controls(cassie, None);
    let mut stream =
        TypedScanStream::open(cassie, Some(session), "facts", &["v".to_owned()], &probe)
            .expect("open")
            .expect("vector source");
    let Some(Partitioning::Parallel(scan)) = stream.partitioning(4).expect("partitioning") else {
        panic!("a segmented table runs in waves");
    };
    (
        probe.current_query_memory_bytes(),
        scan.wave_reservation(size_of::<usize>()).expect("size"),
    )
}

#[test]
fn should_refuse_a_wave_before_any_worker_reads_when_the_budget_is_one_byte_short() {
    // Arrange
    let (cassie, session) = fixture(4);
    insert_rows(&cassie, 0..2_000);
    segment(&cassie);
    let (base, wave) = wave_base_and_reservation(&cassie, &session);
    let short = controls(&cassie, Some(base + wave - 1));
    let before = cassie.midge.column_vector_reads_for_diagnostics();

    // Act
    let refused = wave_run(&cassie, &session, &short);
    let delta = cassie
        .midge
        .column_vector_reads_for_diagnostics()
        .since(before);

    // Assert
    assert!(matches!(
        refused,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(delta.partitions, 0);
    assert_eq!(short.current_query_memory_bytes(), 0);
    assert_eq!(cassie.runtime.snapshot().runtime.active_operator_workers, 0);
}

#[test]
fn should_start_workers_once_the_budget_covers_the_wave_reservation() {
    // Arrange
    let (cassie, session) = fixture(4);
    insert_rows(&cassie, 0..2_000);
    segment(&cassie);
    let (base, wave) = wave_base_and_reservation(&cassie, &session);
    let covered = controls(&cassie, Some(base + wave));
    let ample = controls(&cassie, Some(base + wave + 8 * 1_024 * 1_024));
    let before = cassie.midge.column_vector_reads_for_diagnostics();

    // Act
    let exhausted = wave_run(&cassie, &session, &covered);
    let started = cassie
        .midge
        .column_vector_reads_for_diagnostics()
        .since(before);
    let rows = wave_run(&cassie, &session, &ample);

    // Assert
    assert!(matches!(
        exhausted,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert!(started.partitions > 0, "workers started at the reservation");
    assert_eq!(covered.current_query_memory_bytes(), 0);
    assert_eq!(rows.expect("ample budget"), 2_000);
    assert_eq!(cassie.runtime.snapshot().runtime.active_operator_workers, 0);
}

#[test]
fn should_read_every_partition_from_the_snapshot_the_scan_opened_on() {
    // Arrange
    let (cassie, session) = fixture(2);
    insert_rows(&cassie, 0..2_000);
    segment(&cassie);
    let scan_controls = controls(&cassie, None);
    let mut stream = TypedScanStream::open(
        &cassie,
        Some(&session),
        "facts",
        &["v".to_owned()],
        &scan_controls,
    )
    .expect("open")
    .expect("vector source");
    let Some(Partitioning::Parallel(scan)) = stream.partitioning(2).expect("partitioning") else {
        panic!("a segmented table runs in waves");
    };
    let mut merged = 0;
    let mut counted = 0_usize;

    // Act
    scan.run(
        0,
        |batches| {
            let mut rows = 0_usize;
            while let Some(batch) = batches.next()? {
                rows += batch.len();
            }
            Ok(rows)
        },
        |rows| {
            if merged == 0 {
                // A concurrent commit lands in the last range after the first wave.
                insert_rows(&cassie, 9_000..9_050);
            }
            merged += 1;
            counted += rows;
            Ok(())
        },
    )
    .expect("waves");
    drop(scan);
    drop(stream);

    // Assert
    assert_eq!(counted, 2_000);
    let fresh = served(&cassie, &session, TOTAL, &controls(&cassie, None))
        .expect("fresh run")
        .expect("served");
    assert_eq!(fresh[0].entries()[0].1, Value::Int64(2_050));
}
