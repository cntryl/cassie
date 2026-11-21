//! Synchronized repeatable-read snapshot probes against the event model.
//!
//! Each probe captures the projection checkpoint and its rows inside one
//! repeatable-read snapshot, then compares them with the independent model
//! replayed through exactly that checkpoint, so the verdict does not depend on
//! how far the writer, readers or maintenance have progressed.

use super::{
    model, temporal, thread, value_text, BTreeMap, Cassie, CassieSession, Duration, EventRow,
    Instant, Ordering, ProbeResult, ProbeScope, SharedMetrics, Value, PAGE_ROWS, PROBE_DEVICES,
    TABLE,
};

/// Upper bound for the commit log to catch up with a snapshot's checkpoint. The
/// log is appended right after each commit returns, so this is a hang guard
/// that aborts the run; it is never a pass or fail condition.
const COMMIT_LOG_HANG_GUARD: Duration = Duration::from_secs(600);

/// Releases the reader pause once the snapshot capture statement has returned,
/// including when the probe unwinds.
struct ResumeReaders<'a>(&'a SharedMetrics);

impl Drop for ResumeReaders<'_> {
    fn drop(&mut self) {
        self.0.readers_paused.store(false, Ordering::Release);
    }
}

pub(super) struct PreparedReaderProbe<'a> {
    pub(super) statement_prefix: &'a str,
    pub(super) probe_sequence: u64,
    pub(super) projection: &'a str,
    pub(super) initial_rows: &'a BTreeMap<i64, EventRow>,
    pub(super) shared: &'a SharedMetrics,
    pub(super) devices: &'a [String],
    pub(super) retention_applied: bool,
    pub(super) scope: ProbeScope,
}

/// Devices sampled at one synchronization point: up to `PROBE_DEVICES`, spread
/// evenly across the device space and rotated by the probe sequence so that
/// successive probes cover different devices.
pub(super) fn probe_devices(device_count: usize, sequence: u64) -> Vec<String> {
    let sampled = device_count.min(PROBE_DEVICES);
    let stride = device_count / sampled;
    let rotation = usize::try_from(sequence).unwrap_or_default() % stride;
    (0..sampled)
        .map(|index| model::device_name(index * stride + rotation))
        .collect()
}

/// Opens a repeatable-read snapshot and reads the checkpoint inside it.
///
/// The other readers finish their running statement and idle while the
/// snapshot is captured, because the capture needs the catalog publication
/// guard exclusively and continuously overlapping statements would otherwise
/// keep it from ever being free.
fn open_snapshot(
    cassie: &Cassie,
    session: &CassieSession,
    probe: &PreparedReaderProbe<'_>,
) -> String {
    probe.shared.readers_paused.store(true, Ordering::Release);
    while probe.shared.readers_in_flight.load(Ordering::Acquire) > 0 {
        thread::sleep(Duration::from_millis(1));
    }
    let resume = ResumeReaders(probe.shared);
    execute_checked(
        cassie,
        session,
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
    );
    let checkpoint_result = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT source_checkpoint FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{}'",
                probe.projection
            ),
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("read checkpoint in snapshot: {error}"));
    drop(resume);
    checkpoint_result.rows()[0][0]
        .as_str()
        .expect("projection checkpoint text")
        .to_owned()
}

/// The model replayed through the snapshot's checkpoint.
fn expected_at_checkpoint(
    probe: &PreparedReaderProbe<'_>,
    checkpoint: &str,
) -> BTreeMap<i64, EventRow> {
    let checkpoint_position = model::checkpoint_position(checkpoint)
        .expect("checkpoint string includes monotonically increasing position");
    wait_for_committed_position(probe.shared, checkpoint_position);
    let committed = probe
        .shared
        .committed_events
        .lock()
        .expect("committed event log mutex")
        .clone();
    model::apply_through_checkpoint(
        probe.initial_rows,
        &committed,
        checkpoint_position,
        probe.retention_applied,
    )
}

pub(super) fn verify_snapshot(
    cassie: &Cassie,
    session: &CassieSession,
    probe: &PreparedReaderProbe<'_>,
) -> ProbeResult {
    let checkpoint = open_snapshot(cassie, session, probe);
    let (expected_rows, actual_rows, matched) = match probe.scope {
        ProbeScope::Pages => compare_pages(cassie, session, probe, &checkpoint),
        ProbeScope::FullRelation => compare_relation(cassie, session, probe, &checkpoint),
    };
    ProbeResult {
        checkpoint,
        expected_rows,
        actual_rows,
        matched,
    }
}

fn compare_pages(
    cassie: &Cassie,
    session: &CassieSession,
    probe: &PreparedReaderProbe<'_>,
    checkpoint: &str,
) -> (usize, usize, bool) {
    let windows = (0..probe.devices.len())
        .map(|index| {
            (usize::try_from(probe.probe_sequence).unwrap_or_default() + index)
                % model::RANGE_WINDOWS
        })
        .collect::<Vec<_>>();
    let mut pages = Vec::with_capacity(probe.devices.len());
    for (device, window) in probe.devices.iter().zip(&windows) {
        let page_result = cassie
            .execute_sql(
                session,
                &format!("EXECUTE {}{window}('{device}')", probe.statement_prefix),
                Vec::new(),
            )
            .expect("read prepared page in checkpoint snapshot");
        pages.push(result_event_rows(page_result.rows()));
    }
    execute_checked(cassie, session, "COMMIT");

    let expected_rows = expected_at_checkpoint(probe, checkpoint);
    let mut expected_total = 0;
    let mut actual_total = 0;
    let mut matched = true;
    for ((device, window), actual) in probe.devices.iter().zip(&windows).zip(&pages) {
        let expected = model::oracle_pages(&expected_rows, device, *window, PAGE_ROWS);
        expected_total += expected.len();
        actual_total += actual.len();
        if *actual != expected {
            eprintln!(
                "mixed: oracle page mismatch checkpoint={checkpoint} device={device} window={window} expected={} actual={}",
                expected.len(),
                actual.len(),
            );
        }
        matched &= *actual == expected;
    }
    (expected_total, actual_total, matched)
}

fn compare_relation(
    cassie: &Cassie,
    session: &CassieSession,
    probe: &PreparedReaderProbe<'_>,
    checkpoint: &str,
) -> (usize, usize, bool) {
    let result = cassie
        .execute_sql(
            session,
            &format!("SELECT id,device,event_at,amount FROM {TABLE} ORDER BY id"),
            Vec::new(),
        )
        .expect("read full relation in checkpoint snapshot");
    let actual = result_event_rows(result.rows());
    execute_checked(cassie, session, "COMMIT");

    let expected = expected_at_checkpoint(probe, checkpoint)
        .into_values()
        .collect::<Vec<_>>();
    let matched = actual == expected;
    if !matched {
        eprintln!(
            "mixed: full relation mismatch checkpoint={checkpoint} expected_rows={} actual_rows={}",
            expected.len(),
            actual.len()
        );
    }
    (expected.len(), actual.len(), matched)
}

fn wait_for_committed_position(shared: &SharedMetrics, target: u64) {
    let deadline = Instant::now() + COMMIT_LOG_HANG_GUARD;
    while shared.committed_position.load(Ordering::Acquire) < target {
        assert!(
            Instant::now() < deadline,
            "commit log hang guard: checkpoint position {target} was never recorded"
        );
        thread::yield_now();
    }
}

pub(super) fn execute_checked(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, Vec::new())
        .unwrap_or_else(|error| panic!("execute campaign SQL {sql:?}: {error}"));
}

pub(super) fn result_event_rows(rows: &[Vec<Value>]) -> Vec<EventRow> {
    rows.iter()
        .map(|row| EventRow {
            id: row[0].as_i64().expect("page row id is INT"),
            device: row[1].as_str().expect("page device is TEXT").to_owned(),
            event_at: temporal::canonical_timestamp(&value_text(&row[2]))
                .expect("page timestamp is canonicalizable"),
            amount: row[3].as_i64().expect("page amount is INT"),
        })
        .collect()
}
