//! The mixed window: a phase machine over refresh, backfill and retention.
//!
//! The contract times (minute two, three and five) are release points, not
//! deadlines. A phase never starts before its time, and retention additionally
//! waits for the refresh, the correction drain and every snapshot probe taken
//! before it, so each probe compares against a model whose retention state is
//! known exactly. Phases that are still unfinished when the measured window
//! ends run to completion; nothing is judged by how long it took.

use super::runtime::{MixedWindow, MixedWindowResult};
use super::{
    model, thread, Duration, Instant, MaintenanceAction, MaintenanceResult, MeasuredWindow,
    Ordering, ProbeResult, ProbeScope, Producer, Receiver, SharedMetrics, TryRecvError,
    WindowCounters,
};

/// Aborts a wedged run. It is a hang guard, never a pass or fail condition.
const PHASE_HANG_GUARD: Duration = Duration::from_hours(4);

pub(super) fn run_mixed_window(mut window: MixedWindow<'_>) -> MixedWindowResult {
    let before = WindowCounters::capture(window.shared);
    let producer = Producer::start(
        window.writer.queue.clone(),
        window.cursor.take().expect("mixed event cursor"),
        window.target_rate,
    );
    let mut progress = MixedProgress::default();
    window.shared.readers_active.store(true, Ordering::Release);

    while window.start.elapsed() < window.duration {
        progress.advance(&window, &producer);
        thread::sleep(Duration::from_millis(25));
    }

    let measured_elapsed = window.start.elapsed();
    window.shared.readers_active.store(false, Ordering::Release);
    let after = WindowCounters::capture(window.shared);
    let result = progress.finish(
        &window,
        &producer,
        MeasuredWindow { before, after },
        measured_elapsed,
    );
    producer.stop();
    result
}

/// Where one maintenance or backfill phase is in its lifecycle.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Waiting,
    Running,
    Done,
}

#[derive(Default)]
struct MixedProgress {
    maintenance_actions: Vec<MaintenanceAction>,
    oracle_page_checks: usize,
    oracle_checkpoints: Vec<String>,
    oracle_rows_compared: usize,
    correction_events_expected: u64,
    correction_events_acknowledged: u64,
    refresh: Phase,
    corrections: Phase,
    retention: Phase,
    refresh_receiver: Option<Receiver<MaintenanceResult>>,
    correction_receiver: Option<Receiver<u64>>,
    pending_probes: Vec<Receiver<ProbeResult>>,
    retention_receiver: Option<Receiver<MaintenanceResult>>,
}

impl MixedProgress {
    fn advance(&mut self, window: &MixedWindow<'_>, producer: &Producer) {
        let elapsed = window.start.elapsed();
        self.collect_refresh(window);
        self.collect_corrections(window);
        self.collect_retention(window);
        self.collect_probes(window);
        if self.refresh == Phase::Waiting && elapsed >= model::scaled(model::REFRESH_AT_SECONDS) {
            self.start_refresh(window);
        }
        if self.corrections == Phase::Waiting
            && elapsed >= model::scaled(model::CORRECTION_AT_SECONDS)
        {
            self.start_corrections(window, producer);
        }
        if elapsed >= model::scaled(model::RETENTION_AT_SECONDS) {
            self.start_retention_when_ready(window);
        }
    }

    fn start_refresh(&mut self, window: &MixedWindow<'_>) {
        self.refresh_receiver = Some(window.maintenance.refresh());
        self.refresh = Phase::Running;
    }

    fn start_corrections(&mut self, window: &MixedWindow<'_>, producer: &Producer) {
        self.correction_events_expected = u64::try_from(window.fixture.event_count / 50)
            .expect("two-percent correction count fits u64");
        self.correction_receiver = Some(producer.offer_corrections(window.fixture.event_count));
        self.corrections = Phase::Running;
    }

    /// Retention runs once the model has no other change in flight: the refresh
    /// and the correction drain are complete and their probes have reported.
    fn start_retention_when_ready(&mut self, window: &MixedWindow<'_>) {
        if self.retention != Phase::Waiting
            || self.refresh != Phase::Done
            || self.corrections != Phase::Done
            || !self.pending_probes.is_empty()
        {
            return;
        }
        self.retention = Phase::Running;
        self.retention_receiver = Some(window.maintenance.retention());
    }

    fn collect_refresh(&mut self, window: &MixedWindow<'_>) {
        let Some(receiver) = self.refresh_receiver.as_ref() else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.refresh_receiver = None;
                if result.completed {
                    self.refresh = Phase::Done;
                }
                if result.completed {
                    self.pending_probes
                        .push(window.readers.request_verify(false, ProbeScope::Pages));
                }
                self.maintenance_actions.push(action_record(
                    result,
                    model::scaled_seconds(model::REFRESH_AT_SECONDS),
                    window.start,
                ));
            }
            Err(TryRecvError::Disconnected) => {
                panic!("refresh worker disconnected before reporting completion");
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn collect_corrections(&mut self, window: &MixedWindow<'_>) {
        if let Some(receiver) = self.correction_receiver.as_ref() {
            match receiver.try_recv() {
                Ok(offered) => {
                    assert_eq!(
                        offered, self.correction_events_expected,
                        "correction burst must equal two percent of seeded events"
                    );
                    self.correction_receiver = None;
                }
                Err(TryRecvError::Disconnected) => {
                    panic!("producer disconnected before confirming corrections");
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        // Acknowledgement is read from exact counters; once the whole burst is
        // acknowledged the relation is probed in full for backfill accounting.
        if self.corrections == Phase::Running
            && self.correction_receiver.is_none()
            && window
                .shared
                .acknowledged_correction_events
                .load(Ordering::Acquire)
                >= self.correction_events_expected
        {
            self.correction_events_acknowledged = window
                .shared
                .acknowledged_correction_events
                .load(Ordering::Acquire);
            self.corrections = Phase::Done;
            self.pending_probes
                .push(window.readers.request_verify(false, ProbeScope::Pages));
            self.pending_probes.push(
                window
                    .readers
                    .request_verify(false, ProbeScope::FullRelation),
            );
        }
    }

    fn collect_retention(&mut self, window: &MixedWindow<'_>) {
        let Some(receiver) = self.retention_receiver.as_ref() else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => self.complete_retention(window, result),
            Err(TryRecvError::Disconnected) => {
                panic!("retention worker disconnected before reporting completion");
            }
            Err(TryRecvError::Empty) => {}
        }
    }

    fn complete_retention(&mut self, window: &MixedWindow<'_>, result: MaintenanceResult) {
        if result.completed {
            self.retention = Phase::Done;
        }
        self.maintenance_actions.push(action_record(
            result,
            model::scaled_seconds(model::RETENTION_AT_SECONDS),
            window.start,
        ));
        if self.retention == Phase::Done {
            self.pending_probes
                .push(window.readers.request_verify(true, ProbeScope::Pages));
        }
        self.retention_receiver = None;
    }

    fn collect_probes(&mut self, window: &MixedWindow<'_>) {
        let mut index = 0;
        while index < self.pending_probes.len() {
            match self.pending_probes[index].try_recv() {
                Ok(probe) => {
                    self.pending_probes.swap_remove(index);
                    record_probe(
                        probe,
                        window.shared,
                        &mut self.oracle_page_checks,
                        &mut self.oracle_checkpoints,
                        &mut self.oracle_rows_compared,
                    );
                }
                Err(TryRecvError::Disconnected) => {
                    panic!("prepared reader disconnected before oracle probe result");
                }
                Err(TryRecvError::Empty) => index += 1,
            }
        }
    }

    /// Runs every phase the measured window did not reach to completion while
    /// the producer keeps offering steady batches.
    fn finish(
        mut self,
        window: &MixedWindow<'_>,
        producer: &Producer,
        measured_work: MeasuredWindow,
        measured_elapsed: Duration,
    ) -> MixedWindowResult {
        let hang_guard = Instant::now() + PHASE_HANG_GUARD;
        while self.has_pending_work() {
            assert!(
                Instant::now() < hang_guard,
                "mixed phase hang guard: refresh_done={} corrections_done={} retention_done={} probes={}",
                self.refresh == Phase::Done,
                self.corrections == Phase::Done,
                self.retention == Phase::Done,
                self.pending_probes.len()
            );
            self.collect_refresh(window);
            self.collect_corrections(window);
            self.collect_retention(window);
            self.collect_probes(window);
            if self.refresh == Phase::Waiting {
                self.start_refresh(window);
            }
            if self.corrections == Phase::Waiting {
                self.start_corrections(window, producer);
            }
            self.start_retention_when_ready(window);
            thread::sleep(Duration::from_millis(5));
        }
        MixedWindowResult {
            measured_elapsed,
            maintenance_actions: self.maintenance_actions,
            oracle_page_checks: self.oracle_page_checks,
            oracle_checkpoints: self.oracle_checkpoints,
            oracle_rows_compared: self.oracle_rows_compared,
            correction_events_expected: self.correction_events_expected,
            correction_events_acknowledged: self.correction_events_acknowledged,
            retention_applied: self.retention == Phase::Done,
            window: measured_work,
        }
    }

    fn has_pending_work(&self) -> bool {
        self.refresh != Phase::Done
            || self.corrections != Phase::Done
            || self.retention != Phase::Done
            || !self.pending_probes.is_empty()
    }
}

fn action_record(
    result: MaintenanceResult,
    scheduled_elapsed_seconds: u64,
    start: Instant,
) -> MaintenanceAction {
    MaintenanceAction {
        name: result.name,
        scheduled_elapsed_seconds,
        actual_elapsed_millis: start.elapsed().as_millis(),
        operation_elapsed_millis: result.elapsed.as_millis(),
        completed_batches: result.completed_batches,
        completed: result.completed,
        deleted_rows: result.deleted_rows,
    }
}

fn record_probe(
    probe: ProbeResult,
    shared: &SharedMetrics,
    checks: &mut usize,
    checkpoints: &mut Vec<String>,
    rows_compared: &mut usize,
) {
    *checks = checks.saturating_add(1);
    *rows_compared = rows_compared.saturating_add(probe.expected_rows.max(probe.actual_rows));
    checkpoints.push(probe.checkpoint);
    if !probe.matched {
        shared.oracle_mismatches.fetch_add(1, Ordering::Relaxed);
    }
}
