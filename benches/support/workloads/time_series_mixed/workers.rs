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

#[path = "workers_runtime.rs"]
mod runtime;
use runtime::*;
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
const ORACLE_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Serialize)]
pub struct ScenarioReport {
    pub elapsed: Duration,
    pub completed_operations: u64,
    pub writer_rate: u64,
    pub offered_events: u64,
    pub acknowledged_events: u64,
    pub peak_backlog_events: u64,
    pub backlog_events_at_window_end: u64,
    pub backlog_growth_events: i64,
    pub backlog_stable: bool,
    pub sustainable_rate: u64,
    pub target_writer_rate: u64,
    pub reader_queries: u64,
    pub reader_p50_us: f64,
    pub reader_p95_us: f64,
    pub rollup_debt: bool,
    pub rollup_refresh_completed: bool,
    pub fallback_scans: u64,
    pub rollup_refresh_contended_failures: u64,
    pub rollup_refresh_writer_quiesced_millis: u64,
    pub first_unsustainable_rate: Option<u64>,
    pub sampled_peak_rss_bytes: u64,
    pub correction_events_expected: u64,
    pub correction_events_acknowledged: u64,
    pub pending_retention: bool,
    pub oracle_page_checks: usize,
    pub oracle_checkpoints: Vec<String>,
    pub oracle_rows_compared: usize,
    pub oracle_rollup_groups_compared: usize,
    pub oracle_mismatches: usize,
    pub first_limiting_resource: String,
    pub rate_attempts: Vec<RateAttempt>,
    pub samples: Vec<SecondSample>,
    pub maintenance_actions: Vec<MaintenanceAction>,
    pub raw_reader_latency_us: Vec<u64>,
    pub raw_writer_latency_us: Vec<u64>,
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
}

#[derive(Default)]
struct SharedMetrics {
    offered_events: AtomicU64,
    acknowledged_events: AtomicU64,
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

    fn record_offer(&self, event_count: usize) {
        self.offered_events.fetch_add(
            u64::try_from(event_count).expect("offered events fit u64"),
            Ordering::Relaxed,
        );
        self.observe_backlog();
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
        self.acknowledged_events.fetch_add(
            u64::try_from(mutations.len()).expect("acknowledged events fit u64"),
            Ordering::Relaxed,
        );
        self.observe_backlog();
    }

    fn observe_backlog(&self) {
        let backlog = self
            .offered_events
            .load(Ordering::Relaxed)
            .saturating_sub(self.acknowledged_events.load(Ordering::Relaxed));
        self.peak_backlog_events
            .fetch_max(backlog, Ordering::Relaxed);
        self.window_peak_backlog_events
            .fetch_max(backlog, Ordering::Relaxed);
    }
}

enum ReaderCommand {
    Verify {
        retention_applied: bool,
        response: mpsc::Sender<ProbeResult>,
    },
    Stop,
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
        let join = thread::spawn(move || writer_loop(cassie, projection, receiver, writer_shared));
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
        self.shared.record_offer(mutations.len());
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
        let join = thread::spawn(move || producer_loop(queue, cursor, receiver, target_rate));
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
    fn request_verify(&self, retention_applied: bool) -> Receiver<ProbeResult> {
        let (response, receiver) = mpsc::channel();
        self.probe_sender
            .send(ReaderCommand::Verify {
                retention_applied,
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
                    format!("backfill-correction-{id}-{}", self.next_position),
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
    fixture: &Fixture,
    kind: RunKind,
    duration: Duration,
    sustainable_rate: Option<u64>,
    reader_baseline_p95: Option<f64>,
) -> ScenarioReport {
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
    let mut run = ScenarioRun::start(fixture, kind);
    let outcome = run.execute(fixture, kind, duration, sustainable_rate);
    let elapsed = outcome.measured_elapsed;
    run.stop();
    run.report(fixture, kind, elapsed, reader_baseline_p95, outcome)
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
    backlog_events_at_window_end: Option<u64>,
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
            .then(|| start_readers(fixture, Arc::clone(&shared)));
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
                let started = Instant::now();
                self.shared.readers_active.store(true, Ordering::Release);
                wait_for_duration(duration, started);
                self.shared.readers_active.store(false, Ordering::Release);
                ScenarioOutcome {
                    measured_elapsed: started.elapsed(),
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
        let measured_elapsed = run_paced_window(writer, cursor, sustainable_rate.max(1), duration);
        ScenarioOutcome {
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
            backlog_events_at_window_end: Some(result.backlog_events_at_window_end),
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
    ) -> ScenarioReport {
        let backlog_at_window_end = outcome.backlog_events_at_window_end.unwrap_or_else(|| {
            self.shared
                .offered_events
                .load(Ordering::Acquire)
                .saturating_sub(self.shared.acknowledged_events.load(Ordering::Acquire))
        });
        let final_state = verify_final_mixed_state(
            fixture,
            &self.shared,
            outcome.retention_applied,
            self.fallback_scans_before,
            outcome
                .maintenance_actions
                .iter()
                .any(|action| action.name == "rollup_refresh" && action.completed),
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
    let reader_queries = shared.reader_queries.load(Ordering::Acquire);
    let samples = shared.samples.lock().expect("sample vector mutex").clone();
    let reader_latency_ns = shared
        .reader_latency_ns
        .lock()
        .expect("reader latency mutex")
        .clone();
    let writer_latency_ns = shared
        .writer_latency_ns
        .lock()
        .expect("writer latency mutex")
        .clone();
    let reader_p50_us = percentile_us(&reader_latency_ns, 5_000);
    let reader_p95_us = percentile_us(&reader_latency_ns, 9_500);
    let backlog_stable = backlog_is_stable(kind, backlog_events_at_window_end, &samples);
    let writer_rate = writer_rate(kind, outcome.sustainable_rate, acknowledged_events, elapsed);
    let peak_backlog_events = shared.peak_backlog_events.load(Ordering::Acquire);
    let sampled_peak_rss_bytes = samples
        .iter()
        .map(|sample| sample.sampled_rss_bytes)
        .max()
        .unwrap_or_default()
        .max(super::evidence::process_rss_bytes());
    let first_limiting_resource = infer_limiting_resource(
        kind,
        backlog_events_at_window_end,
        &outcome.rate_attempts,
        reader_p95_us,
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
    let completed_operations = acknowledged_events.saturating_add(reader_queries).max(1);
    let oracle_mismatches = final_state.oracle_mismatches.saturating_add(
        usize::try_from(shared.oracle_mismatches.load(Ordering::Acquire)).unwrap_or(usize::MAX),
    );

    ScenarioReport {
        elapsed,
        completed_operations,
        writer_rate,
        offered_events,
        acknowledged_events,
        peak_backlog_events,
        backlog_events_at_window_end,
        backlog_growth_events: backlog_growth(&samples),
        backlog_stable,
        sustainable_rate: outcome.sustainable_rate,
        target_writer_rate: outcome.target_writer_rate,
        reader_queries,
        reader_p50_us,
        reader_p95_us,
        rollup_debt: final_state.rollup_debt,
        rollup_refresh_completed,
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
        pending_retention,
        oracle_page_checks: outcome.oracle_page_checks,
        oracle_checkpoints: outcome.oracle_checkpoints,
        oracle_rows_compared: outcome
            .oracle_rows_compared
            .saturating_add(final_state.oracle_rows_compared),
        oracle_rollup_groups_compared: final_state.oracle_rollup_groups_compared,
        oracle_mismatches,
        first_limiting_resource,
        rate_attempts: outcome.rate_attempts,
        samples,
        maintenance_actions: outcome.maintenance_actions,
        raw_reader_latency_us: latency_samples_us(&reader_latency_ns),
        raw_writer_latency_us: latency_samples_us(&writer_latency_ns),
    }
}

fn writer_rate(kind: RunKind, sustainable_rate: u64, acknowledged: u64, elapsed: Duration) -> u64 {
    match kind {
        RunKind::IngestOnly => sustainable_rate,
        RunKind::ReaderOnly => 0,
        RunKind::Mixed => {
            let elapsed_nanos = elapsed.as_nanos();
            if elapsed_nanos > 0 {
                let rate = u128::from(acknowledged).saturating_mul(1_000_000_000) / elapsed_nanos;
                u64::try_from(rate).unwrap_or(u64::MAX)
            } else {
                0
            }
        }
    }
}

fn latency_samples_us(latencies_ns: &[u128]) -> Vec<u64> {
    latencies_ns
        .iter()
        .map(|latency| u64::try_from(latency / 1_000).unwrap_or(u64::MAX))
        .collect()
}
