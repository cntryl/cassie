use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::{temporal, Value};
use serde::Serialize;

use super::fixture::{Fixture, DAILY_ROLLUP, RETENTION_POLICY, TABLE};
use super::model::{self, EventRow, Mutation};
use super::RunKind;

#[path = "workers_probe.rs"]
mod probe;
use probe::{
    probe_devices, range_page_sql, read_checkpoint, read_relation_pages, result_event_rows,
    verify_snapshot, PreparedReaderProbe,
};
#[path = "workers_oracle_controls.rs"]
mod oracle_controls;
use oracle_controls::should_bound_window_counters;
#[path = "workers_retention.rs"]
mod retention;
#[path = "workers_runtime.rs"]
mod runtime;
use retention::{enforce_retention, retention_accounting, retention_pending};
use runtime::{
    backlog_growth, backlog_is_stable, bracket_ingest_rate, current_fallback_scans,
    infer_limiting_resource, percentile_us, producer_loop, run_paced_window, start_maintenance,
    start_readers, start_sampler, value_text, verify_final_mixed_state, wait_for_duration,
    wait_for_writer_drain, writer_loop, FinalStateEvidence, MixedWindow,
};
#[path = "workers_mixed.rs"]
mod mixed;
use mixed::run_mixed_window;

const READER_COUNT: usize = 4;
const PAGE_ROWS: usize = 128;
const PROBE_DEVICES: usize = 16;
const WRITER_QUEUE_BATCHES: usize = 32;
const BRACKET_FIRST_RATE: u64 = 128;
const BRACKET_REFINEMENTS: usize = 2;
const BRACKET_MAX_RATE: u64 = 65_536;
const BRACKET_WINDOW: Duration = Duration::from_secs(8);
const WRITER_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Serialize)]
pub struct ScenarioReport {
    pub elapsed: Duration,
    pub completed_operations: u64,
    pub writer_rate: u64,
    pub offered_events: u64,
    pub acknowledged_events: u64,
    pub window_offered_events: u64,
    pub window_acknowledged_events: u64,
    pub peak_backlog_events: u64,
    pub backlog_events_at_window_end: u64,
    pub backlog_growth_events: i64,
    pub backlog_stable: bool,
    pub sustainable_rate: u64,
    pub target_writer_rate: u64,
    pub reader_queries: u64,
    pub reader_p50_us: f64,
    pub reader_p95_us: f64,
    #[serde(flatten)]
    pub maintenance: MaintenanceFlags,
    pub fallback_scans: u64,
    pub rollup_refresh_contended_failures: u64,
    pub rollup_refresh_writer_quiesced_millis: u64,
    pub first_unsustainable_rate: Option<u64>,
    pub sampled_peak_rss_bytes: u64,
    pub correction_events_expected: u64,
    pub correction_events_acknowledged: u64,
    pub oracle_page_checks: usize,
    pub oracle_checkpoints: Vec<String>,
    pub oracle_rows_compared: usize,
    pub oracle_rollup_groups_compared: usize,
    pub reopened_checkpoint: String,
    pub reopened_page_checks: usize,
    pub oracle_mismatches: usize,
    pub retention_expected_deleted_rows: u64,
    pub retention_reported_deleted_rows: u64,
    pub retention_expired_rows_remaining: u64,
    pub first_limiting_resource: String,
    pub rate_attempts: Vec<RateAttempt>,
    pub samples: Vec<SecondSample>,
    pub maintenance_actions: Vec<MaintenanceAction>,
    pub raw_reader_latency_us: Vec<u64>,
    pub raw_writer_latency_us: Vec<u64>,
}

/// Maintenance outcomes, grouped so the report struct stays under the boolean
/// field budget; serialized flat.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct MaintenanceFlags {
    pub rollup_debt: bool,
    pub rollup_refresh_completed: bool,
    pub pending_retention: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RateAttempt {
    pub target_events_per_second: u64,
    pub offered_events_per_second: u64,
    pub window_elapsed_millis: u128,
    pub offered_events: u64,
    pub acknowledged_events_at_window_end: u64,
    pub backlog_events_at_window_end: u64,
    pub peak_backlog_events: u64,
    pub drained_within_timeout: bool,
    pub sustainable: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SecondSample {
    pub elapsed_seconds: u64,
    pub offered_events: u64,
    pub acknowledged_events: u64,
    pub offered_events_per_second: u64,
    pub acknowledged_events_per_second: u64,
    pub backlog_events: u64,
    pub reader_queries: u64,
    pub sampled_rss_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MaintenanceAction {
    pub name: String,
    pub scheduled_elapsed_seconds: u64,
    pub actual_elapsed_millis: u128,
    pub operation_elapsed_millis: u128,
    pub completed_batches: u64,
    pub completed: bool,
    pub deleted_rows: u64,
}

#[derive(Default)]
struct SharedMetrics {
    offered_events: AtomicU64,
    acknowledged_events: AtomicU64,
    /// Correction-burst events offered and acknowledged. The burst lands as one
    /// 2% step by contract, so it is accounted separately from the steady
    /// ingest backlog.
    offered_correction_events: AtomicU64,
    acknowledged_correction_events: AtomicU64,
    peak_backlog_events: AtomicU64,
    window_peak_backlog_events: AtomicU64,
    reader_queries: AtomicU64,
    readers_active: AtomicBool,
    /// Raised for the instant a synchronized oracle probe captures its
    /// repeatable-read snapshot; readers finish their statement and idle.
    readers_paused: AtomicBool,
    readers_in_flight: AtomicU64,
    oracle_mismatches: AtomicU64,
    refresh_contended_failures: AtomicU64,
    refresh_writer_quiesced_millis: AtomicU64,
    /// Held by the writer around every commit; a full rollup refresh takes it to
    /// hold the source still, because the refresh never publishes against a
    /// source that keeps committing.
    writer_gate: Mutex<()>,
    committed_position: AtomicU64,
    running: AtomicBool,
    committed_events: Mutex<Vec<Mutation>>,
    writer_latency_ns: Mutex<Vec<u128>>,
    reader_latency_ns: Mutex<Vec<u128>>,
    samples: Mutex<Vec<SecondSample>>,
}

impl SharedMetrics {
    fn with_initial_position(position: u64, readers_active: bool) -> Self {
        Self {
            committed_position: AtomicU64::new(position),
            running: AtomicBool::new(true),
            readers_active: AtomicBool::new(readers_active),
            ..Self::default()
        }
    }

    fn record_offer(&self, mutations: &[Mutation]) {
        self.offered_events.fetch_add(
            u64::try_from(mutations.len()).expect("offered events fit u64"),
            Ordering::Relaxed,
        );
        self.offered_correction_events
            .fetch_add(correction_events(mutations), Ordering::Relaxed);
        self.observe_backlog();
    }

    /// Steady-ingest backlog: offered minus acknowledged events, excluding the
    /// correction burst, which is verified by exact drain accounting instead.
    fn steady_backlog(&self) -> u64 {
        let offered = self
            .offered_events
            .load(Ordering::Acquire)
            .saturating_sub(self.offered_correction_events.load(Ordering::Acquire));
        let acknowledged = self
            .acknowledged_events
            .load(Ordering::Acquire)
            .saturating_sub(self.acknowledged_correction_events.load(Ordering::Acquire));
        offered.saturating_sub(acknowledged)
    }

    fn record_commit(&self, mutations: &[Mutation], latency: Duration) {
        self.committed_events
            .lock()
            .expect("committed event log mutex")
            .extend_from_slice(mutations);
        if let Some(last) = mutations.last() {
            self.committed_position
                .store(last.position, Ordering::Release);
        }
        self.writer_latency_ns
            .lock()
            .expect("writer latency mutex")
            .push(latency.as_nanos());
        self.acknowledged_correction_events
            .fetch_add(correction_events(mutations), Ordering::Relaxed);
        self.acknowledged_events.fetch_add(
            u64::try_from(mutations.len()).expect("acknowledged events fit u64"),
            Ordering::Release,
        );
        self.observe_backlog();
    }

    fn observe_backlog(&self) {
        let backlog = self.steady_backlog();
        self.peak_backlog_events
            .fetch_max(backlog, Ordering::Relaxed);
        self.window_peak_backlog_events
            .fetch_max(backlog, Ordering::Relaxed);
    }
}

fn correction_events(mutations: &[Mutation]) -> u64 {
    u64::try_from(
        mutations
            .iter()
            .filter(|mutation| {
                mutation
                    .event_id
                    .starts_with(model::CORRECTION_EVENT_PREFIX)
            })
            .count(),
    )
    .expect("correction count fits u64")
}

enum ReaderCommand {
    Verify {
        retention_applied: bool,
        scope: ProbeScope,
        response: mpsc::Sender<ProbeResult>,
    },
    Stop,
}

/// What a synchronized snapshot probe compares with the event model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeScope {
    /// Sampled prepared range pages, the shape the readers run.
    Pages,
    /// Every row of the relation, for exact backfill accounting.
    FullRelation,
}

#[derive(Clone, Debug)]
struct ProbeResult {
    checkpoint: String,
    expected_rows: usize,
    actual_rows: usize,
    matched: bool,
}

enum MaintenanceCommand {
    Refresh(mpsc::Sender<MaintenanceResult>),
    Retention(mpsc::Sender<MaintenanceResult>),
    Stop,
}

#[derive(Clone, Debug)]
struct MaintenanceResult {
    name: String,
    elapsed: Duration,
    completed_batches: u64,
    completed: bool,
    /// Rows the operation reports as removed; zero for non-retention work.
    deleted_rows: u64,
}

struct WriterService {
    queue: WriterQueue,
    join: Option<JoinHandle<()>>,
}

#[derive(Clone)]
struct WriterQueue {
    sender: SyncSender<Vec<Mutation>>,
    shared: Arc<SharedMetrics>,
}

impl WriterService {
    fn start(cassie: Arc<Cassie>, projection: String, shared: Arc<SharedMetrics>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(WRITER_QUEUE_BATCHES);
        let writer_shared = Arc::clone(&shared);
        let join =
            thread::spawn(move || writer_loop(&cassie, &projection, &receiver, &writer_shared));
        Self {
            queue: WriterQueue { sender, shared },
            join: Some(join),
        }
    }

    fn offer(&self, mutations: Vec<Mutation>) {
        self.queue.offer(mutations);
    }

    fn close(mut self) {
        drop(self.queue);
        self.join
            .take()
            .expect("writer thread handle")
            .join()
            .expect("writer thread completed");
    }
}

impl WriterQueue {
    fn offer(&self, mutations: Vec<Mutation>) {
        assert!(!mutations.is_empty(), "writer batches cannot be empty");
        self.shared.record_offer(&mutations);
        self.sender
            .send(mutations)
            .expect("send offered projection batch to the writer");
    }
}

struct EventCursor {
    next_position: u64,
    next_row_id: u64,
    device_count: usize,
}

enum ProducerCommand {
    Corrections {
        seed_events: usize,
        response: mpsc::Sender<u64>,
    },
    Stop,
}

struct Producer {
    sender: mpsc::Sender<ProducerCommand>,
    join: Option<JoinHandle<()>>,
}

impl Producer {
    fn start(queue: WriterQueue, cursor: EventCursor, target_rate: u64) -> Self {
        let (sender, receiver) = mpsc::channel();
        let join = thread::spawn(move || producer_loop(&queue, cursor, &receiver, target_rate));
        Self {
            sender,
            join: Some(join),
        }
    }

    fn offer_corrections(&self, seed_events: usize) -> mpsc::Receiver<u64> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(ProducerCommand::Corrections {
                seed_events,
                response,
            })
            .expect("request deterministic correction burst");
        receiver
    }

    fn stop(mut self) {
        let _ = self.sender.send(ProducerCommand::Stop);
        self.join
            .take()
            .expect("producer thread handle")
            .join()
            .expect("producer thread completed");
    }
}

struct ReaderPool {
    probe_sender: mpsc::Sender<ReaderCommand>,
    joins: Vec<JoinHandle<()>>,
}

impl ReaderPool {
    fn request_verify(&self, retention_applied: bool, scope: ProbeScope) -> Receiver<ProbeResult> {
        let (response, receiver) = mpsc::channel();
        self.probe_sender
            .send(ReaderCommand::Verify {
                retention_applied,
                scope,
                response,
            })
            .expect("send synchronized oracle page probe");
        receiver
    }

    fn stop_and_join(self) {
        let _ = self.probe_sender.send(ReaderCommand::Stop);
        for join in self.joins {
            join.join().expect("prepared reader thread completed");
        }
    }
}

struct MaintenanceWorker {
    sender: mpsc::Sender<MaintenanceCommand>,
    join: Option<JoinHandle<()>>,
}

impl MaintenanceWorker {
    fn refresh(&self) -> mpsc::Receiver<MaintenanceResult> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(MaintenanceCommand::Refresh(response))
            .expect("request scheduled rollup refresh");
        receiver
    }

    fn retention(&self) -> mpsc::Receiver<MaintenanceResult> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(MaintenanceCommand::Retention(response))
            .expect("request scheduled retention enforcement");
        receiver
    }

    fn stop_and_join(mut self) {
        let _ = self.sender.send(MaintenanceCommand::Stop);
        self.join
            .take()
            .expect("maintenance thread handle")
            .join()
            .expect("maintenance thread completed");
    }
}

impl EventCursor {
    fn new(seed_events: usize, device_count: usize) -> Self {
        let seed_position = u64::try_from(seed_events).expect("seed events fit u64");
        Self {
            next_position: seed_position + 1,
            next_row_id: seed_position + 1,
            device_count,
        }
    }

    fn steady_batch(&mut self) -> Vec<Mutation> {
        (0..model::WRITER_BATCH_EVENTS)
            .map(|_| {
                let row_id = self.next_row_id;
                self.next_row_id = self.next_row_id.checked_add(1).expect("row id overflow");
                self.next_mutation(
                    model::new_row(row_id, self.device_count),
                    format!("steady-event-{row_id}"),
                )
            })
            .collect()
    }

    fn correction_burst(&mut self, seed_events: usize) -> Vec<Mutation> {
        (50..=seed_events)
            .step_by(50)
            .map(|row_id| {
                let id = u64::try_from(row_id).expect("correction row id fits u64");
                self.next_mutation(
                    model::correction(row_id, self.device_count),
                    format!(
                        "{}{id}-{}",
                        model::CORRECTION_EVENT_PREFIX,
                        self.next_position
                    ),
                )
            })
            .collect()
    }

    fn next_mutation(&mut self, row: EventRow, event_id: String) -> Mutation {
        let position = self.next_position;
        self.next_position = self
            .next_position
            .checked_add(1)
            .expect("source position overflow");
        model::mutation(row, position, event_id)
    }
}

pub fn run_scenario(
    fixture: Fixture,
    kind: RunKind,
    duration: Duration,
    sustainable_rate: Option<u64>,
    reader_baseline_p95: Option<f64>,
) -> (Fixture, ScenarioReport) {
    if matches!(kind, RunKind::Mixed) {
        assert!(
            sustainable_rate.is_some_and(|rate| rate > 0),
            "mixed profile requires a nonzero sustainable ingest-only bracket"
        );
        assert!(
            reader_baseline_p95.is_some_and(|p95| p95 > 0.0),
            "mixed profile requires a measured reader-only p95 baseline"
        );
    }
    let mut run = ScenarioRun::start(&fixture, kind);
    let outcome = run.execute(&fixture, kind, duration, sustainable_rate);
    let elapsed = outcome.measured_elapsed;
    run.stop();
    let fallback_scans =
        current_fallback_scans(&fixture.cassie).saturating_sub(run.fallback_scans_before);
    let reopened = fixture.reopen();
    let report = run.report(
        &reopened,
        kind,
        elapsed,
        reader_baseline_p95,
        outcome,
        fallback_scans,
    );
    (reopened, report)
}

/// Counters captured while opening or closing the measured window.
#[derive(Default)]
struct WindowCounters {
    offered: u64,
    acknowledged: u64,
    reader_samples: usize,
    writer_samples: usize,
    backlog: u64,
}

impl WindowCounters {
    fn capture(shared: &SharedMetrics) -> Self {
        // Read completions first. A concurrent offer can be pending at the
        // boundary, but its acknowledgement cannot precede its offer.
        let acknowledged = shared.acknowledged_events.load(Ordering::Acquire);
        Self {
            offered: shared.offered_events.load(Ordering::Acquire),
            acknowledged,
            reader_samples: shared
                .reader_latency_ns
                .lock()
                .expect("reader latency mutex")
                .len(),
            writer_samples: shared
                .writer_latency_ns
                .lock()
                .expect("writer latency mutex")
                .len(),
            backlog: shared.steady_backlog(),
        }
    }
}

#[derive(Default)]
struct MeasuredWindow {
    before: WindowCounters,
    after: WindowCounters,
}

#[derive(Default)]
struct ScenarioOutcome {
    measured_elapsed: Duration,
    sustainable_rate: u64,
    target_writer_rate: u64,
    rate_attempts: Vec<RateAttempt>,
    maintenance_actions: Vec<MaintenanceAction>,
    oracle_page_checks: usize,
    oracle_checkpoints: Vec<String>,
    oracle_rows_compared: usize,
    correction_events_expected: u64,
    correction_events_acknowledged: u64,
    retention_applied: bool,
    window: MeasuredWindow,
}

struct ScenarioRun {
    shared: Arc<SharedMetrics>,
    writer: Option<WriterService>,
    readers: Option<ReaderPool>,
    maintenance: Option<MaintenanceWorker>,
    sampler: Option<JoinHandle<()>>,
    cursor: Option<EventCursor>,
    fallback_scans_before: u64,
}

impl ScenarioRun {
    fn start(fixture: &Fixture, kind: RunKind) -> Self {
        let fallback_scans_before = current_fallback_scans(&fixture.cassie);
        let shared = Arc::new(SharedMetrics::with_initial_position(
            u64::try_from(fixture.event_count).expect("seed position fits u64"),
            false,
        ));
        let writer = matches!(kind, RunKind::IngestOnly | RunKind::Mixed).then(|| {
            WriterService::start(
                Arc::clone(&fixture.cassie),
                fixture.projection.clone(),
                Arc::clone(&shared),
            )
        });
        let readers = matches!(kind, RunKind::ReaderOnly | RunKind::Mixed)
            .then(|| start_readers(fixture, &shared));
        let maintenance =
            matches!(kind, RunKind::Mixed).then(|| start_maintenance(fixture, Arc::clone(&shared)));
        let sampler = Some(start_sampler(Arc::clone(&shared)));
        let cursor = Some(EventCursor::new(fixture.event_count, fixture.device_count));
        Self {
            shared,
            writer,
            readers,
            maintenance,
            sampler,
            cursor,
            fallback_scans_before,
        }
    }

    fn execute(
        &mut self,
        fixture: &Fixture,
        kind: RunKind,
        duration: Duration,
        sustainable_rate: Option<u64>,
    ) -> ScenarioOutcome {
        match kind {
            RunKind::IngestOnly => self.execute_ingest(duration),
            RunKind::ReaderOnly => {
                let before = WindowCounters::capture(&self.shared);
                let started = Instant::now();
                self.shared.readers_active.store(true, Ordering::Release);
                wait_for_duration(duration, started);
                self.shared.readers_active.store(false, Ordering::Release);
                ScenarioOutcome {
                    measured_elapsed: started.elapsed(),
                    window: MeasuredWindow {
                        before,
                        after: WindowCounters::capture(&self.shared),
                    },
                    ..ScenarioOutcome::default()
                }
            }
            RunKind::Mixed => self.execute_mixed(fixture, duration, sustainable_rate),
        }
    }

    fn execute_ingest(&mut self, duration: Duration) -> ScenarioOutcome {
        let writer = self.writer.as_ref().expect("ingest writer");
        let cursor = self.cursor.as_mut().expect("ingest event cursor");
        let (sustainable_rate, rate_attempts) = bracket_ingest_rate(writer, cursor);
        if sustainable_rate == 0 {
            return ScenarioOutcome {
                rate_attempts,
                ..ScenarioOutcome::default()
            };
        }
        assert!(
            wait_for_writer_drain(&self.shared, WRITER_DRAIN_TIMEOUT),
            "ingest bracket must drain before opening the measurement window"
        );
        let before = WindowCounters::capture(&self.shared);
        let measured_elapsed = run_paced_window(writer, cursor, sustainable_rate.max(1), duration);
        let after = WindowCounters::capture(&self.shared);
        ScenarioOutcome {
            window: MeasuredWindow { before, after },
            measured_elapsed,
            sustainable_rate,
            target_writer_rate: sustainable_rate,
            rate_attempts,
            ..ScenarioOutcome::default()
        }
    }

    fn execute_mixed(
        &mut self,
        fixture: &Fixture,
        duration: Duration,
        sustainable_rate: Option<u64>,
    ) -> ScenarioOutcome {
        let rate = sustainable_rate.expect("mixed scenario follows ingest-only bracket");
        let target_rate = (rate / 2).max(1);
        let result = run_mixed_window(MixedWindow {
            fixture,
            writer: self.writer.as_ref().expect("mixed writer"),
            readers: self.readers.as_ref().expect("mixed readers"),
            maintenance: self.maintenance.as_ref().expect("mixed maintenance worker"),
            shared: &self.shared,
            cursor: Some(self.cursor.take().expect("mixed event cursor")),
            target_rate,
            duration,
            start: Instant::now(),
        });
        ScenarioOutcome {
            measured_elapsed: result.measured_elapsed,
            sustainable_rate: rate,
            target_writer_rate: target_rate,
            maintenance_actions: result.maintenance_actions,
            oracle_page_checks: result.oracle_page_checks,
            oracle_checkpoints: result.oracle_checkpoints,
            oracle_rows_compared: result.oracle_rows_compared,
            correction_events_expected: result.correction_events_expected,
            correction_events_acknowledged: result.correction_events_acknowledged,
            retention_applied: result.retention_applied,
            window: result.window,
            ..ScenarioOutcome::default()
        }
    }

    fn stop(&mut self) {
        self.shared.running.store(false, Ordering::Release);
        if let Some(readers) = self.readers.take() {
            readers.stop_and_join();
        }
        if let Some(maintenance) = self.maintenance.take() {
            maintenance.stop_and_join();
        }
        if let Some(writer) = self.writer.take() {
            writer.close();
        }
        if let Some(sampler) = self.sampler.take() {
            sampler.join().expect("sampling thread completed");
        }
    }

    fn report(
        &self,
        fixture: &Fixture,
        kind: RunKind,
        elapsed: Duration,
        reader_baseline_p95: Option<f64>,
        outcome: ScenarioOutcome,
        fallback_scans: u64,
    ) -> ScenarioReport {
        let backlog_at_window_end = outcome.window.after.backlog;
        let final_state = verify_final_mixed_state(
            fixture,
            &self.shared,
            outcome.retention_applied,
            fallback_scans,
            outcome
                .maintenance_actions
                .iter()
                .any(|action| action.name == "rollup_refresh" && action.completed),
            outcome
                .maintenance_actions
                .iter()
                .filter(|action| action.name == "retention_enforcement" && action.completed)
                .map(|action| action.deleted_rows)
                .sum(),
        );
        let rollup_refresh_completed = outcome
            .maintenance_actions
            .iter()
            .any(|action| action.name == "rollup_refresh" && action.completed);
        let pending_retention =
            retention_pending(fixture).expect("read final scheduled retention state");
        build_report(
            &self.shared,
            ReportInput {
                kind,
                elapsed,
                reader_baseline_p95,
                outcome,
                backlog_events_at_window_end: backlog_at_window_end,
                final_state,
                pending_retention,
                rollup_refresh_completed,
            },
        )
    }
}

struct ReportInput {
    kind: RunKind,
    elapsed: Duration,
    reader_baseline_p95: Option<f64>,
    outcome: ScenarioOutcome,
    backlog_events_at_window_end: u64,
    final_state: FinalStateEvidence,
    pending_retention: bool,
    rollup_refresh_completed: bool,
}

fn build_report(shared: &SharedMetrics, input: ReportInput) -> ScenarioReport {
    let ReportInput {
        kind,
        elapsed,
        reader_baseline_p95,
        outcome,
        backlog_events_at_window_end,
        final_state,
        pending_retention,
        rollup_refresh_completed,
    } = input;
    let offered_events = shared.offered_events.load(Ordering::Acquire);
    let acknowledged_events = shared.acknowledged_events.load(Ordering::Acquire);
    let window = &outcome.window;
    let window_offered_events = window.after.offered - window.before.offered;
    let window_acknowledged_events = window.after.acknowledged - window.before.acknowledged;
    should_bound_window_counters(
        window_offered_events,
        window_acknowledged_events,
        offered_events,
        acknowledged_events,
    );
    let samples = shared.samples.lock().expect("sample vector mutex").clone();
    let latencies = capture_window_latencies(shared, window);
    let backlog_stable = backlog_is_stable(kind, backlog_events_at_window_end, &samples);
    let writer_rate = writer_rate(window_acknowledged_events, elapsed);
    let peak_backlog_events = shared.peak_backlog_events.load(Ordering::Acquire);
    let sampled_peak_rss_bytes = sampled_peak_rss_bytes(&samples);
    let first_limiting_resource = infer_limiting_resource(
        kind,
        backlog_events_at_window_end,
        &outcome.rate_attempts,
        latencies.reader_p95_us,
        reader_baseline_p95,
        final_state.rollup_debt,
        final_state.fallback_scans,
    );
    let first_unsustainable_rate = outcome
        .rate_attempts
        .iter()
        .find(|attempt| !attempt.sustainable)
        .map(|attempt| attempt.target_events_per_second);
    let correction_events_expected = outcome.correction_events_expected;
    let correction_events_acknowledged = outcome.correction_events_acknowledged;
    let completed_operations = window_acknowledged_events.saturating_add(latencies.reader_queries);
    let oracle_mismatches = final_state.oracle_mismatches.saturating_add(
        usize::try_from(shared.oracle_mismatches.load(Ordering::Acquire)).unwrap_or(usize::MAX),
    );

    ScenarioReport {
        elapsed,
        completed_operations,
        writer_rate,
        offered_events,
        acknowledged_events,
        window_offered_events,
        window_acknowledged_events,
        peak_backlog_events,
        backlog_events_at_window_end,
        backlog_growth_events: backlog_growth(&samples),
        backlog_stable,
        sustainable_rate: outcome.sustainable_rate,
        target_writer_rate: outcome.target_writer_rate,
        reader_queries: latencies.reader_queries,
        reader_p50_us: latencies.reader_p50_us,
        reader_p95_us: latencies.reader_p95_us,
        maintenance: MaintenanceFlags {
            rollup_debt: final_state.rollup_debt,
            rollup_refresh_completed,
            pending_retention,
        },
        fallback_scans: final_state.fallback_scans,
        rollup_refresh_contended_failures: shared
            .refresh_contended_failures
            .load(Ordering::Acquire),
        rollup_refresh_writer_quiesced_millis: shared
            .refresh_writer_quiesced_millis
            .load(Ordering::Acquire),
        first_unsustainable_rate,
        sampled_peak_rss_bytes,
        correction_events_expected,
        correction_events_acknowledged,
        oracle_page_checks: outcome.oracle_page_checks,
        oracle_checkpoints: outcome.oracle_checkpoints,
        oracle_rows_compared: outcome
            .oracle_rows_compared
            .saturating_add(final_state.oracle_rows_compared),
        oracle_rollup_groups_compared: final_state.oracle_rollup_groups_compared,
        reopened_checkpoint: final_state.reopened_checkpoint,
        reopened_page_checks: final_state.reopened_page_checks,
        oracle_mismatches,
        retention_expected_deleted_rows: final_state.retention_expected_deleted_rows,
        retention_reported_deleted_rows: final_state.retention_reported_deleted_rows,
        retention_expired_rows_remaining: final_state.retention_expired_rows_remaining,
        first_limiting_resource,
        rate_attempts: outcome.rate_attempts,
        samples,
        maintenance_actions: outcome.maintenance_actions,
        raw_reader_latency_us: latency_samples_us(&latencies.reader_latency_ns),
        raw_writer_latency_us: latency_samples_us(&latencies.writer_latency_ns),
    }
}

struct WindowLatencies {
    reader_latency_ns: Vec<u128>,
    writer_latency_ns: Vec<u128>,
    reader_queries: u64,
    reader_p50_us: f64,
    reader_p95_us: f64,
}

fn capture_window_latencies(shared: &SharedMetrics, window: &MeasuredWindow) -> WindowLatencies {
    let reader_latency_ns = shared
        .reader_latency_ns
        .lock()
        .expect("reader latency mutex")[window.before.reader_samples..window.after.reader_samples]
        .to_vec();
    let reader_queries = u64::try_from(reader_latency_ns.len()).expect("reader count fits u64");
    let writer_latency_ns = shared
        .writer_latency_ns
        .lock()
        .expect("writer latency mutex")[window.before.writer_samples..window.after.writer_samples]
        .to_vec();
    let reader_p50_us = percentile_us(&reader_latency_ns, 5_000);
    let reader_p95_us = percentile_us(&reader_latency_ns, 9_500);
    WindowLatencies {
        reader_latency_ns,
        writer_latency_ns,
        reader_queries,
        reader_p50_us,
        reader_p95_us,
    }
}

fn sampled_peak_rss_bytes(samples: &[SecondSample]) -> u64 {
    samples
        .iter()
        .map(|sample| sample.sampled_rss_bytes)
        .max()
        .unwrap_or_default()
        .max(super::evidence::process_rss_bytes())
}

fn writer_rate(acknowledged: u64, elapsed: Duration) -> u64 {
    u128::from(acknowledged)
        .saturating_mul(1_000_000_000)
        .checked_div(elapsed.as_nanos())
        .map_or(0, |rate| u64::try_from(rate).unwrap_or(u64::MAX))
}

fn latency_samples_us(latencies_ns: &[u128]) -> Vec<u64> {
    latencies_ns
        .iter()
        .map(|latency| u64::try_from(latency / 1_000).unwrap_or(u64::MAX))
        .collect()
}
