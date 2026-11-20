use super::runtime::{MixedWindow, MixedWindowResult};
use super::*;

pub(super) fn run_mixed_window(mut window: MixedWindow<'_>) -> MixedWindowResult {
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
    let backlog_events_at_window_end = window
        .shared
        .offered_events
        .load(Ordering::Acquire)
        .saturating_sub(window.shared.acknowledged_events.load(Ordering::Acquire));
    producer.stop();
    progress.finish(&window, backlog_events_at_window_end, measured_elapsed)
}

#[derive(Default)]
struct MixedProgress {
    maintenance_actions: Vec<MaintenanceAction>,
    oracle_page_checks: usize,
    oracle_checkpoints: Vec<String>,
    oracle_rows_compared: usize,
    correction_events_expected: u64,
    correction_events_acknowledged: u64,
    retention_applied: bool,
    refresh_started: bool,
    retention_started: bool,
    corrections_started: bool,
    refresh_receiver: Option<Receiver<MaintenanceResult>>,
    correction_receiver: Option<Receiver<u64>>,
    correction_offer_confirmed: bool,
    pending_probes: Vec<Receiver<ProbeResult>>,
    retention_receiver: Option<Receiver<MaintenanceResult>>,
}

impl MixedProgress {
    fn advance(&mut self, window: &MixedWindow<'_>, producer: &Producer) {
        let elapsed = window.start.elapsed();
        if !self.refresh_started && elapsed >= model::scaled(model::REFRESH_AT_SECONDS) {
            self.start_refresh(window);
        }
        if !self.corrections_started && elapsed >= model::scaled(model::CORRECTION_AT_SECONDS) {
            self.start_corrections(window, producer);
        }
        if !self.retention_started && elapsed >= model::scaled(model::RETENTION_AT_SECONDS) {
            self.retention_started = true;
            self.retention_receiver = Some(window.maintenance.retention());
        }
        self.collect_refresh(window);
        self.collect_corrections(window);
        self.collect_retention(window);
        self.collect_probes(window);
    }

    fn start_refresh(&mut self, window: &MixedWindow<'_>) {
        self.refresh_receiver = Some(window.maintenance.refresh());
        self.refresh_started = true;
    }

    fn start_corrections(&mut self, window: &MixedWindow<'_>, producer: &Producer) {
        self.correction_events_expected = u64::try_from(window.fixture.event_count / 50)
            .expect("two-percent correction count fits u64");
        self.correction_receiver = Some(producer.offer_corrections(window.fixture.event_count));
        self.corrections_started = true;
    }

    fn collect_refresh(&mut self, window: &MixedWindow<'_>) {
        let Some(receiver) = self.refresh_receiver.as_ref() else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.refresh_receiver = None;
                if result.completed {
                    self.pending_probes
                        .push(window.readers.request_verify(false));
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
                    self.correction_offer_confirmed = true;
                }
                Err(TryRecvError::Disconnected) => {
                    panic!("producer disconnected before confirming corrections");
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.correction_offer_confirmed
            && self.correction_events_acknowledged == 0
            && committed_correction_count(window.shared) >= self.correction_events_expected
        {
            self.correction_events_acknowledged = committed_correction_count(window.shared);
            self.pending_probes
                .push(window.readers.request_verify(false));
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
        self.retention_applied = result.completed;
        self.maintenance_actions.push(action_record(
            result,
            model::scaled_seconds(model::RETENTION_AT_SECONDS),
            window.start,
        ));
        if self.retention_applied {
            self.pending_probes
                .push(window.readers.request_verify(true));
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

    fn finish(
        mut self,
        window: &MixedWindow<'_>,
        backlog_events_at_window_end: u64,
        measured_elapsed: Duration,
    ) -> MixedWindowResult {
        let completion_deadline = Instant::now() + Duration::from_secs(180);
        while self.has_pending_work() && Instant::now() < completion_deadline {
            self.collect_refresh(window);
            self.collect_corrections(window);
            self.collect_retention(window);
            self.collect_probes(window);
            thread::sleep(Duration::from_millis(5));
        }
        if self.refresh_receiver.is_some() {
            self.maintenance_actions.push(incomplete_action(
                "rollup_refresh",
                model::scaled_seconds(model::REFRESH_AT_SECONDS),
                window.start,
            ));
        }
        if self.retention_receiver.is_some() {
            self.maintenance_actions.push(incomplete_action(
                "retention_enforcement",
                model::scaled_seconds(model::RETENTION_AT_SECONDS),
                window.start,
            ));
        }
        MixedWindowResult {
            measured_elapsed,
            maintenance_actions: self.maintenance_actions,
            oracle_page_checks: self.oracle_page_checks,
            oracle_checkpoints: self.oracle_checkpoints,
            oracle_rows_compared: self.oracle_rows_compared,
            correction_events_expected: self.correction_events_expected,
            correction_events_acknowledged: self.correction_events_acknowledged,
            retention_applied: self.retention_applied,
            backlog_events_at_window_end,
        }
    }

    fn has_pending_work(&self) -> bool {
        self.refresh_receiver.is_some()
            || self.correction_receiver.is_some()
            || (self.corrections_started
                && self.correction_events_acknowledged < self.correction_events_expected)
            || self.retention_receiver.is_some()
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
    }
}

fn incomplete_action(
    name: &str,
    scheduled_elapsed_seconds: u64,
    start: Instant,
) -> MaintenanceAction {
    MaintenanceAction {
        name: name.to_owned(),
        scheduled_elapsed_seconds,
        actual_elapsed_millis: start.elapsed().as_millis(),
        operation_elapsed_millis: 180_000,
        completed_batches: 0,
        completed: false,
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

fn committed_correction_count(shared: &SharedMetrics) -> u64 {
    u64::try_from(
        shared
            .committed_events
            .lock()
            .expect("committed event log mutex")
            .iter()
            .filter(|mutation| mutation.event_id.starts_with("backfill-correction-"))
            .count(),
    )
    .expect("committed corrections fit u64")
}
