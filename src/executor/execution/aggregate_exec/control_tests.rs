use std::path::PathBuf;
use std::sync::{mpsc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::*;
use crate::app::CassieError;
use crate::config::CassieRuntimeConfig;
use crate::runtime::QueryCancellationHandle;

struct Fixture {
    cassie: Cassie,
    path: PathBuf,
}

impl Fixture {
    fn new(timeout_ms: u64) -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-aggregate-phase-controls-{}",
            uuid::Uuid::new_v4()
        ));
        let config = CassieRuntimeConfig {
            limits: CassieRuntimeLimits {
                parallel_aggregation_workers: 2,
                query_timeout_ms: timeout_ms,
                ..CassieRuntimeLimits::default()
            },
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("engine");
        Self { cassie, path }
    }

    fn assert_released_permits(&self) {
        assert_eq!(
            self.cassie
                .runtime
                .snapshot()
                .runtime
                .active_operator_workers,
            0
        );
        let replacement = self
            .cassie
            .runtime
            .try_acquire_operator_workers(2)
            .expect("worker permits must be reusable");
        assert_eq!(replacement.workers(), 2);
        drop(replacement);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Default)]
struct PhaseGate {
    released: Mutex<bool>,
    wake: Condvar,
}

impl PhaseGate {
    fn wait(&self) {
        let released = self.released.lock().expect("phase gate");
        drop(
            self.wake
                .wait_while(released, |released| !*released)
                .expect("released phase gate"),
        );
    }

    fn release(&self) {
        *self.released.lock().expect("phase gate") = true;
        self.wake.notify_all();
    }
}

struct ReleaseOnDrop<'a>(&'a PhaseGate);

impl Drop for ReleaseOnDrop<'_> {
    fn drop(&mut self) {
        self.0.release();
    }
}

#[derive(Clone, Copy)]
enum ActiveAction {
    Cancel,
    Expire,
    Finish,
}

const ACTIVE_ROW_COUNT: usize = 2 * batch::DEFAULT_BATCH_SIZE;

fn partition_rows(with_canary: bool) -> Vec<BatchRow> {
    (0..ACTIVE_ROW_COUNT)
        .map(|index| {
            let partition_row = index % (ACTIVE_ROW_COUNT / 2);
            let score = if with_canary && partition_row == 1 {
                Value::String("late SUM canary".to_owned())
            } else {
                Value::Int64(1)
            };
            BatchRow::new(vec![
                (
                    "tenant".to_owned(),
                    Value::String(format!("tenant-{}", index / (ACTIVE_ROW_COUNT / 2))),
                ),
                ("score".to_owned(), score),
            ])
        })
        .collect()
}

fn run_active_workers(action: ActiveAction) {
    let timeout_ms = if matches!(action, ActiveAction::Expire) {
        2_000
    } else {
        0
    };
    let fixture = Fixture::new(timeout_ms);
    let plan = aggregate_plan("SELECT tenant, SUM(score) AS total FROM events GROUP BY tenant");
    let rows = partition_rows(!matches!(action, ActiveAction::Finish));
    let functions = HashMap::new();
    let cancellation = QueryCancellationHandle::new();
    let gate = PhaseGate::default();
    let (ready, activation) = mpsc::channel();
    let observer = |processed_rows| {
        if processed_rows == 1 {
            ready.send(()).expect("report active worker");
            gate.wait();
        }
    };
    let controls = QueryExecutionControls::with_cancellation(
        &fixture.cassie.runtime.limits(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut context = aggregate_context(&plan, &functions, &controls);
    context.after_partition_row = Some(&observer);
    let before = fixture.cassie.metrics();
    let result = thread::scope(|scope| {
        // Release blocked workers before scoped joining if an activation assertion fails.
        let release = ReleaseOnDrop(&gate);
        let worker =
            scope.spawn(|| aggregate_query_batches_inner(&fixture.cassie, vec![rows], &context));
        for _ in 0..2 {
            activation
                .recv_timeout(Duration::from_secs(5))
                .expect("both partitions must retain their first group");
        }
        assert_eq!(
            fixture
                .cassie
                .runtime
                .snapshot()
                .runtime
                .active_operator_workers,
            2
        );
        assert!(controls.current_query_memory_bytes() > 0);
        assert!(!controls.is_cancelled());
        assert!(!controls.is_timed_out());
        match action {
            ActiveAction::Cancel => cancellation.cancel(),
            ActiveAction::Expire => {
                let deadline = controls.deadline.expect("configured deadline");
                thread::sleep(deadline.saturating_duration_since(Instant::now()));
                assert!(controls.is_timed_out());
            }
            ActiveAction::Finish => assert!(controls.deadline.is_none()),
        }
        drop(release);
        worker.join().expect("aggregate orchestration worker")
    });
    let after = fixture.cassie.metrics();
    assert_eq!(controls.current_query_memory_bytes(), 0);
    fixture.assert_released_permits();
    assert_active_result(action, result, &before, &after);
}

fn assert_active_result(
    action: ActiveAction,
    result: Result<Vec<Batch>, QueryError>,
    before: &serde_json::Value,
    after: &serde_json::Value,
) {
    match action {
        ActiveAction::Cancel | ActiveAction::Expire => {
            let error = CassieError::from(result.expect_err("active worker controls must stop"));
            assert_eq!(error.descriptor().sql_state, "57014");
            assert!(match action {
                ActiveAction::Cancel => matches!(error, CassieError::QueryCancelled),
                ActiveAction::Expire => matches!(error, CassieError::DeadlineExceeded),
                ActiveAction::Finish => unreachable!("terminal action branch"),
            });
            assert_eq!(
                after["parallel_aggregation"]["aggregations"],
                before["parallel_aggregation"]["aggregations"]
            );
        }
        ActiveAction::Finish => {
            let batches = result.expect("deadline-disabled aggregation");
            let rows = batch::flatten_batches(batches);
            let per_group = i64::try_from(ACTIVE_ROW_COUNT / 2).expect("fixture count");
            assert_eq!(rows.len(), 2);
            assert!(rows
                .iter()
                .all(|row| row.get("total") == Some(&Value::Int64(per_group))));
            assert_eq!(
                after["parallel_aggregation"]["aggregations"].as_u64(),
                Some(
                    before["parallel_aggregation"]["aggregations"]
                        .as_u64()
                        .unwrap()
                        + 1
                )
            );
        }
    }
}

#[test]
fn should_cancel_active_aggregate_workers_with_complete_cleanup() {
    // Arrange
    let action = ActiveAction::Cancel;

    // Act
    run_active_workers(action);

    // Assert
    // The probe checks activation, cancellation precedence, failed-path metrics and cleanup.
}

#[test]
fn should_expire_active_aggregate_workers_with_complete_cleanup() {
    // Arrange
    let action = ActiveAction::Expire;

    // Act
    run_active_workers(action);

    // Assert
    // The initially live deadline expires while both retained worker groups are held.
}

#[test]
fn should_complete_active_aggregate_workers_with_deadlines_disabled() {
    // Arrange
    let action = ActiveAction::Finish;

    // Act
    run_active_workers(action);

    // Assert
    // The probe compares exact grouped counts and releases all shared state.
}

#[test]
fn should_reject_cancelled_aggregate_merge_without_publishing_success() {
    // Arrange
    let fixture = Fixture::new(0);
    let plan = aggregate_plan("SELECT tenant, COUNT(*) AS total FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let functions = HashMap::new();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &fixture.cassie.runtime.limits(),
        Instant::now(),
        cancellation.clone(),
    );
    let context = aggregate_context(&plan, &functions, &controls);
    let rows = partition_rows(false);
    let worker_guard = fixture
        .cassie
        .runtime
        .try_acquire_operator_workers(2)
        .expect("caller worker permits");
    let partials = aggregate_partitions(&rows, &specs, &context, 2).expect("real worker partials");
    assert_eq!(partials.len(), 2);
    assert!(controls.current_query_memory_bytes() > 0);
    assert_eq!(
        fixture
            .cassie
            .runtime
            .snapshot()
            .runtime
            .active_operator_workers,
        2
    );
    let before = fixture.cassie.metrics();

    // Act
    cancellation.cancel();
    let result = merge_partial_aggregations(&fixture.cassie, partials, 2, &rows, &specs, &context);
    let after = fixture.cassie.metrics();
    drop(worker_guard);

    // Assert
    assert_eq!(controls.current_query_memory_bytes(), 0);
    fixture.assert_released_permits();
    // The ordinary SQL caller has a downstream window fence; this asserts the aggregate's
    // own checkpoint and metrics contract, not cancelled SQL row publication.
    assert_eq!(
        after["parallel_aggregation"]["aggregations"],
        before["parallel_aggregation"]["aggregations"],
        "cancelled merge must not report successful aggregation"
    );
    let error = CassieError::from(result.expect_err("cancelled aggregate merge"));
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(error.descriptor().sql_state, "57014");
}

fn single_partial_bytes(
    rows: &[BatchRow],
    specs: &[AggregateSpec],
    plan: &LogicalPlan,
    functions: &HashMap<String, FunctionMeta>,
) -> usize {
    let controls = controls_with_budget(CassieRuntimeLimits::default().query_memory_budget_bytes);
    let context = aggregate_context(plan, functions, &controls);
    let partial = aggregate_partition(rows, specs, &context).expect("one partial fits");
    let bytes = controls.current_query_memory_bytes();
    assert!(bytes > 0);
    drop(partial);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    bytes
}

fn probe_overlapping_partial_budget(exact_budget: bool) {
    let fixture = Fixture::new(0);
    let plan = aggregate_plan("SELECT tenant, MAX(payload) AS widest FROM events GROUP BY tenant");
    let specs = aggregate_specs(&plan);
    let functions = HashMap::new();
    let first_rows = [payload_row("tenant-a", "x".repeat(1_024))];
    let second_rows = [payload_row("tenant-b", "y".repeat(1_024))];
    let first_bytes = single_partial_bytes(&first_rows, &specs, &plan, &functions);
    let second_bytes = single_partial_bytes(&second_rows, &specs, &plan, &functions);
    let combined = first_bytes
        .checked_add(second_bytes)
        .expect("combined budget");
    let budget = if exact_budget { combined } else { combined - 1 };
    let controls = controls_with_budget(budget);
    let context = aggregate_context(&plan, &functions, &controls);
    let worker_guard = fixture
        .cassie
        .runtime
        .try_acquire_operator_workers(2)
        .expect("caller worker permits");
    let gate = PhaseGate::default();
    let (ready, activation) = mpsc::channel();

    thread::scope(|scope| {
        let release = ReleaseOnDrop(&gate);
        let first_worker = scope.spawn(|| {
            let partial = aggregate_partition(&first_rows, &specs, &context)
                .expect("first worker fits individually");
            ready.send(()).expect("report retained partial");
            gate.wait();
            partial
        });
        activation
            .recv_timeout(Duration::from_secs(5))
            .expect("first worker must retain its result");
        assert_eq!(controls.current_query_memory_bytes(), first_bytes);
        assert_eq!(
            fixture
                .cassie
                .runtime
                .snapshot()
                .runtime
                .active_operator_workers,
            2
        );
        let second = scope
            .spawn(|| aggregate_partition(&second_rows, &specs, &context))
            .join()
            .expect("second aggregate worker");
        if exact_budget {
            let partial = match second {
                Ok(partial) => partial,
                Err(error) => panic!("the exact combined budget must fit: {error}"),
            };
            assert_eq!(controls.current_query_memory_bytes(), combined);
            drop(partial);
        } else {
            let error = match second {
                Err(error) => CassieError::from(error),
                Ok(_) => panic!("individually fitting partials must share the combined budget"),
            };
            assert!(matches!(error, CassieError::ResourceLimit(_)));
            assert_eq!(error.descriptor().sql_state, "54000");
        }
        assert_eq!(controls.current_query_memory_bytes(), first_bytes);
        drop(release);
        drop(first_worker.join().expect("first aggregate worker"));
    });
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(worker_guard);
    fixture.assert_released_permits();
}

#[test]
fn should_share_memory_between_individually_fitting_aggregate_worker_results() {
    // Arrange
    let exact_budget = false;

    // Act
    probe_overlapping_partial_budget(exact_budget);

    // Assert
    // The real second partial fails while the first worker's result keeps its reservation.
}

#[test]
fn should_retain_overlapping_aggregate_worker_results_at_the_exact_combined_budget() {
    // Arrange
    let exact_budget = true;

    // Act
    probe_overlapping_partial_budget(exact_budget);

    // Assert
    // Both real partials coexist at the exact bound and release their accounted bytes.
}
