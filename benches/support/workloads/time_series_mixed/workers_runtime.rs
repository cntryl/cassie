use super::*;

pub(super) fn writer_loop(
    cassie: Arc<Cassie>,
    projection: String,
    receiver: Receiver<Vec<Mutation>>,
    shared: Arc<SharedMetrics>,
) {
    while let Ok(mutations) = receiver.recv() {
        let batch_number = shared.acknowledged_events.load(Ordering::Relaxed);
        let last_position = mutations
            .last()
            .map(|mutation| mutation.position)
            .expect("writer receives nonempty batch");
        let batch = model::replay_batch(
            &projection,
            &format!("mixed-batch-{batch_number:012}-{last_position:012}"),
            &mutations,
        );
        let started = Instant::now();
        let commit_gate = shared.writer_gate.lock().expect("writer gate mutex");
        cassie
            .replay_projection_batch_strict(batch)
            .unwrap_or_else(|error| panic!("strict projection writer failed: {error}"));
        drop(commit_gate);
        shared.record_commit(&mutations, started.elapsed());
    }
}

pub(super) fn producer_loop(
    queue: WriterQueue,
    mut cursor: EventCursor,
    receiver: Receiver<ProducerCommand>,
    target_rate: u64,
) {
    let interval = batch_interval(target_rate);
    let mut next_batch = Instant::now();
    loop {
        let wait = next_batch
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(100));
        match receiver.recv_timeout(wait) {
            Ok(ProducerCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(ProducerCommand::Corrections {
                seed_events,
                response,
            }) => {
                let mutations = cursor.correction_burst(seed_events);
                let offered = u64::try_from(mutations.len()).expect("correction count fits u64");
                for batch in mutations.chunks(model::WRITER_BATCH_EVENTS) {
                    queue.offer(batch.to_vec());
                }
                response
                    .send(offered)
                    .expect("correction requester receives offered count");
                next_batch = Instant::now() + interval;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if Instant::now() >= next_batch {
                    queue.offer(cursor.steady_batch());
                    next_batch = Instant::now() + interval;
                }
            }
        }
    }
}

pub(super) fn start_readers(fixture: &Fixture, shared: Arc<SharedMetrics>) -> ReaderPool {
    let (probe_sender, probe_receiver) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::channel();
    let mut probe_receiver = Some(probe_receiver);
    let mut joins = Vec::with_capacity(READER_COUNT);
    let device_count = fixture.device_count;
    for reader_id in 0..READER_COUNT {
        let cassie = Arc::clone(&fixture.cassie);
        let projection = fixture.projection.clone();
        let initial_rows = Arc::clone(&fixture.initial_rows);
        let reader_shared = Arc::clone(&shared);
        let reader_ready = ready_sender.clone();
        let commands = (reader_id == 0).then(|| {
            probe_receiver
                .take()
                .expect("reader zero owns synchronized probe channel")
        });
        joins.push(thread::spawn(move || {
            reader_loop(ReaderWorker {
                cassie,
                projection,
                initial_rows,
                shared: reader_shared,
                reader_id,
                device_count,
                commands,
                ready: reader_ready,
            });
        }));
    }
    drop(ready_sender);
    for _ in 0..READER_COUNT {
        if ready_receiver
            .recv_timeout(Duration::from_secs(60))
            .is_err()
        {
            shared.running.store(false, Ordering::Release);
            let _ = probe_sender.send(ReaderCommand::Stop);
            for join in joins {
                let _ = join.join();
            }
            panic!("prepared reader startup deadline");
        }
    }
    ReaderPool {
        probe_sender,
        joins,
    }
}

struct ReaderWorker {
    cassie: Arc<Cassie>,
    projection: String,
    initial_rows: Arc<BTreeMap<i64, EventRow>>,
    shared: Arc<SharedMetrics>,
    reader_id: usize,
    device_count: usize,
    commands: Option<Receiver<ReaderCommand>>,
    ready: mpsc::Sender<()>,
}

fn reader_loop(worker: ReaderWorker) {
    let ReaderWorker {
        cassie,
        projection,
        initial_rows,
        shared,
        reader_id,
        device_count,
        commands,
        ready,
    } = worker;
    let session = cassie.create_session("benchmark-reader", None);
    let statement = |window: usize| format!("mixed_range_reader_{reader_id}_w{window}");
    for window in 0..model::RANGE_WINDOWS {
        let (start, end) = model::window_bounds(window);
        let prepare = format!(
            "PREPARE {} (TEXT) AS SELECT id,device,event_at,amount FROM {TABLE} WHERE device = $1 AND event_at >= '{start}' AND event_at < '{end}' ORDER BY event_at,id LIMIT {PAGE_ROWS}",
            statement(window)
        );
        cassie
            .execute_sql(&session, &prepare, Vec::new())
            .expect("prepare range reader statement");
    }
    let _ = ready.send(());
    let mut query_sequence = 0_u64;

    while shared.running.load(Ordering::Acquire) {
        if let Some(commands) = commands.as_ref() {
            match commands.try_recv() {
                Ok(ReaderCommand::Stop) | Err(TryRecvError::Disconnected) => break,
                Ok(ReaderCommand::Verify {
                    retention_applied,
                    response,
                }) => {
                    let devices = probe_devices(device_count, query_sequence);
                    query_sequence = query_sequence.saturating_add(1);
                    let probe = verify_prepared_pages(
                        &cassie,
                        &session,
                        PreparedReaderProbe {
                            statement_prefix: &format!("mixed_range_reader_{reader_id}_w"),
                            probe_sequence: query_sequence,
                            projection: &projection,
                            initial_rows: &initial_rows,
                            shared: &shared,
                            devices: &devices,
                            retention_applied,
                        },
                    );
                    response
                        .send(probe)
                        .expect("campaign receives oracle page probe result");
                    continue;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if !shared.readers_active.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        shared.readers_in_flight.fetch_add(1, Ordering::AcqRel);
        if shared.readers_paused.load(Ordering::Acquire) {
            shared.readers_in_flight.fetch_sub(1, Ordering::AcqRel);
            thread::sleep(Duration::from_millis(1));
            continue;
        }
        let device = reader_device(reader_id, query_sequence, device_count);
        let window = usize::try_from(query_sequence).unwrap_or_default() % model::RANGE_WINDOWS;
        query_sequence = query_sequence.saturating_add(1);
        let started = Instant::now();
        let result = cassie
            .execute_sql(
                &session,
                &format!("EXECUTE {}('{device}')", statement(window)),
                Vec::new(),
            )
            .expect("execute prepared time-series page");
        assert!(result.rows().len() <= PAGE_ROWS, "prepared page row cap");
        shared
            .reader_latency_ns
            .lock()
            .expect("reader latency mutex")
            .push(started.elapsed().as_nanos());
        shared.reader_queries.fetch_add(1, Ordering::Relaxed);
        shared.readers_in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Releases the reader pause once the snapshot capture statement has returned,
/// including when the probe unwinds.
struct ResumeReaders<'a>(&'a SharedMetrics);

impl Drop for ResumeReaders<'_> {
    fn drop(&mut self) {
        self.0.readers_paused.store(false, Ordering::Release);
    }
}

struct PreparedReaderProbe<'a> {
    statement_prefix: &'a str,
    probe_sequence: u64,
    projection: &'a str,
    initial_rows: &'a BTreeMap<i64, EventRow>,
    shared: &'a SharedMetrics,
    devices: &'a [String],
    retention_applied: bool,
}

/// Devices sampled at one synchronization point: up to `PROBE_DEVICES`, spread
/// evenly across the device space and rotated by the probe sequence so that
/// successive probes cover different devices.
fn probe_devices(device_count: usize, sequence: u64) -> Vec<String> {
    let sampled = device_count.min(PROBE_DEVICES);
    let stride = device_count / sampled;
    let rotation = usize::try_from(sequence).unwrap_or_default() % stride;
    (0..sampled)
        .map(|index| model::device_name(index * stride + rotation))
        .collect()
}

fn verify_prepared_pages(
    cassie: &Cassie,
    session: &CassieSession,
    probe: PreparedReaderProbe<'_>,
) -> ProbeResult {
    let probe_started = Instant::now();
    // Synchronized pause: the other readers finish their running statement and
    // idle while the snapshot is captured, because the capture needs the
    // catalog publication guard exclusively and continuously overlapping
    // statements would otherwise keep it from ever being free.
    probe.shared.readers_paused.store(true, Ordering::Release);
    while probe.shared.readers_in_flight.load(Ordering::Acquire) > 0 {
        thread::sleep(Duration::from_millis(1));
    }
    execute_checked(
        cassie,
        session,
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
    );
    eprintln!(
        "mixed: probe snapshot opened after {:?}",
        probe_started.elapsed()
    );
    let _resume = ResumeReaders(probe.shared);
    let checkpoint_result = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT source_checkpoint FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{}'",
                probe.projection
            ),
            Vec::new(),
        )
        .unwrap_or_else(|error| {
            panic!(
                "read checkpoint in prepared page snapshot after {:?}: {error}",
                probe_started.elapsed()
            )
        });
    drop(_resume);
    eprintln!(
        "mixed: probe checkpoint read after {:?}",
        probe_started.elapsed()
    );
    let checkpoint = checkpoint_result.rows()[0][0]
        .as_str()
        .expect("projection checkpoint text")
        .to_owned();
    let mut pages = Vec::with_capacity(probe.devices.len());
    let windows = (0..probe.devices.len())
        .map(|index| {
            (usize::try_from(probe.probe_sequence).unwrap_or_default() + index)
                % model::RANGE_WINDOWS
        })
        .collect::<Vec<_>>();
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

    let checkpoint_position = model::checkpoint_position(&checkpoint)
        .expect("checkpoint string includes monotonically increasing position");
    wait_for_committed_position(probe.shared, checkpoint_position);
    let committed = probe
        .shared
        .committed_events
        .lock()
        .expect("committed event log mutex")
        .clone();
    let expected_rows = model::apply_through_checkpoint(
        probe.initial_rows,
        &committed,
        checkpoint_position,
        probe.retention_applied,
    );
    let mut expected_total = 0;
    let mut actual_total = 0;
    let mut matched = true;
    for ((device, window), actual) in probe.devices.iter().zip(&windows).zip(&pages) {
        let expected = model::oracle_pages(&expected_rows, device, *window, PAGE_ROWS);
        expected_total += expected.len();
        actual_total += actual.len();
        if *actual != expected {
            eprintln!(
                "mixed: oracle page mismatch checkpoint={checkpoint} device={device} window={window} expected={} actual={} first_expected={:?} first_actual={:?}",
                expected.len(),
                actual.len(),
                expected.first(),
                actual.first()
            );
        }
        matched &= *actual == expected;
    }
    ProbeResult {
        checkpoint,
        expected_rows: expected_total,
        actual_rows: actual_total,
        matched,
    }
}

fn reader_device(reader_id: usize, sequence: u64, device_count: usize) -> String {
    let step =
        usize::try_from(sequence % u64::try_from(device_count).expect("device count fits u64"))
            .expect("reader device offset fits usize");
    model::device_name((reader_id + step * READER_COUNT) % device_count)
}

pub(super) fn bracket_ingest_rate(
    writer: &WriterService,
    cursor: &mut EventCursor,
) -> (u64, Vec<RateAttempt>) {
    let mut attempts = Vec::new();
    let mut sustainable = 0;
    let mut unsustainable = None;
    let mut target_rate = BRACKET_FIRST_RATE;
    while target_rate <= BRACKET_MAX_RATE {
        let attempt = bracket_attempt(writer, cursor, target_rate);
        let passed = attempt.sustainable;
        attempts.push(attempt);
        if !passed {
            unsustainable = Some(target_rate);
            break;
        }
        sustainable = target_rate;
        target_rate = target_rate.saturating_mul(2);
    }
    // Two bisection steps tighten the doubling bracket around the limit.
    if let (true, Some(mut high)) = (sustainable > 0, unsustainable) {
        for _ in 0..BRACKET_REFINEMENTS {
            let middle = sustainable + (high - sustainable) / 2;
            if middle <= sustainable {
                break;
            }
            let attempt = bracket_attempt(writer, cursor, middle);
            let passed = attempt.sustainable;
            attempts.push(attempt);
            if passed {
                sustainable = middle;
            } else {
                high = middle;
            }
        }
    }
    (sustainable, attempts)
}

fn bracket_attempt(
    writer: &WriterService,
    cursor: &mut EventCursor,
    target_rate: u64,
) -> RateAttempt {
    let offered_before = writer.queue.shared.offered_events.load(Ordering::Acquire);
    let acknowledged_before = writer
        .queue
        .shared
        .acknowledged_events
        .load(Ordering::Acquire);
    writer
        .queue
        .shared
        .window_peak_backlog_events
        .store(0, Ordering::Release);
    let window_elapsed = run_paced_window(writer, cursor, target_rate, BRACKET_WINDOW);
    let offered_at_end = writer.queue.shared.offered_events.load(Ordering::Acquire);
    let acknowledged_at_end = writer
        .queue
        .shared
        .acknowledged_events
        .load(Ordering::Acquire);
    let backlog_at_end = offered_at_end.saturating_sub(acknowledged_at_end);
    let peak_backlog = writer
        .queue
        .shared
        .window_peak_backlog_events
        .load(Ordering::Acquire);
    let drained = wait_for_writer_drain(&writer.queue.shared, WRITER_DRAIN_TIMEOUT);
    let offered = offered_at_end.saturating_sub(offered_before);
    let acknowledged = acknowledged_at_end.saturating_sub(acknowledged_before);
    let elapsed_nanos = window_elapsed.as_nanos().max(1);
    let offered_events_per_second =
        u64::try_from(u128::from(offered).saturating_mul(1_000_000_000) / elapsed_nanos)
            .unwrap_or(u64::MAX);
    let offered_rate_sustainable = u128::from(offered)
        .saturating_mul(1_000_000_000)
        .saturating_mul(100)
        >= u128::from(target_rate)
            .saturating_mul(elapsed_nanos)
            .saturating_mul(95);
    let sustainable_attempt = drained
        && offered_rate_sustainable
        && backlog_at_end
            <= u64::try_from(model::WRITER_BATCH_EVENTS * 2).expect("backlog threshold fits u64")
        && acknowledged.saturating_mul(100) >= offered.saturating_mul(98);
    RateAttempt {
        target_events_per_second: target_rate,
        offered_events_per_second,
        window_elapsed_millis: window_elapsed.as_millis(),
        offered_events: offered,
        acknowledged_events_at_window_end: acknowledged,
        backlog_events_at_window_end: backlog_at_end,
        peak_backlog_events: peak_backlog,
        drained_within_timeout: drained,
        sustainable: sustainable_attempt,
    }
}

pub(super) fn run_paced_window(
    writer: &WriterService,
    cursor: &mut EventCursor,
    target_rate: u64,
    duration: Duration,
) -> Duration {
    let started = Instant::now();
    let interval = batch_interval(target_rate);
    let mut next_batch = started;
    while started.elapsed() < duration {
        if Instant::now() >= next_batch {
            writer.offer(cursor.steady_batch());
            next_batch = Instant::now() + interval;
        } else {
            thread::sleep(
                next_batch
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(5)),
            );
        }
    }
    started.elapsed()
}

fn wait_for_writer_drain(shared: &SharedMetrics, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let backlog = shared
            .offered_events
            .load(Ordering::Acquire)
            .saturating_sub(shared.acknowledged_events.load(Ordering::Acquire));
        if backlog == 0 {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    shared.offered_events.load(Ordering::Acquire)
        == shared.acknowledged_events.load(Ordering::Acquire)
}

fn batch_interval(target_rate: u64) -> Duration {
    let numerator = u128::try_from(model::WRITER_BATCH_EVENTS)
        .expect("writer batch size fits u128")
        .saturating_mul(1_000_000_000);
    let divisor = u128::from(target_rate.max(1));
    let nanos = u64::try_from((numerator / divisor).max(1)).unwrap_or(u64::MAX);
    Duration::from_nanos(nanos)
}

pub(super) fn wait_for_duration(duration: Duration, start: Instant) {
    while start.elapsed() < duration {
        thread::sleep(
            duration
                .saturating_sub(start.elapsed())
                .min(Duration::from_millis(100)),
        );
    }
}

pub(super) fn start_sampler(shared: Arc<SharedMetrics>) -> JoinHandle<()> {
    thread::spawn(move || {
        let started = Instant::now();
        let mut previous = Counters::capture(&shared);
        loop {
            thread::sleep(Duration::from_secs(1));
            let now = Counters::capture(&shared);
            let sample = SecondSample {
                elapsed_seconds: u64::try_from(started.elapsed().as_secs()).unwrap_or(u64::MAX),
                offered_events: now.offered,
                acknowledged_events: now.acknowledged,
                offered_events_per_second: now.offered.saturating_sub(previous.offered),
                acknowledged_events_per_second: now
                    .acknowledged
                    .saturating_sub(previous.acknowledged),
                backlog_events: now.offered.saturating_sub(now.acknowledged),
                reader_queries: now.reader_queries,
                sampled_rss_bytes: crate::workload::evidence::process_rss_bytes(),
            };
            shared
                .samples
                .lock()
                .expect("sample vector mutex")
                .push(sample);
            previous = now;
            if !shared.running.load(Ordering::Acquire) {
                break;
            }
        }
    })
}

#[derive(Clone, Copy)]
struct Counters {
    offered: u64,
    acknowledged: u64,
    reader_queries: u64,
}

impl Counters {
    fn capture(shared: &SharedMetrics) -> Self {
        Self {
            offered: shared.offered_events.load(Ordering::Acquire),
            acknowledged: shared.acknowledged_events.load(Ordering::Acquire),
            reader_queries: shared.reader_queries.load(Ordering::Acquire),
        }
    }
}

pub(super) fn start_maintenance(
    fixture: &Fixture,
    shared: Arc<SharedMetrics>,
) -> MaintenanceWorker {
    let (sender, receiver) = mpsc::channel();
    let cassie = Arc::clone(&fixture.cassie);
    let join = thread::spawn(move || {
        let session = cassie.create_session("benchmark-maintenance", None);
        while let Ok(command) = receiver.recv() {
            match command {
                MaintenanceCommand::Stop => break,
                MaintenanceCommand::Refresh(response) => {
                    let _ = response.send(refresh_rollup(&cassie, &session, &shared));
                }
                MaintenanceCommand::Retention(response) => {
                    let result = enforce_retention(&cassie, &session);
                    let _ = response.send(result);
                }
            }
        }
    });
    MaintenanceWorker {
        sender,
        join: Some(join),
    }
}

/// Refreshes the rollup. The first attempts run while the writer keeps
/// committing; the engine rejects a refresh whose source moved underneath it
/// with a documented retry error, so those attempts starve. The worker then
/// quiesces the writer between commits for one attempt, which publishes, and
/// records both the contended failures and the quiesced time.
/// `completed_batches` counts attempts.
fn refresh_rollup(
    cassie: &Cassie,
    session: &CassieSession,
    shared: &SharedMetrics,
) -> MaintenanceResult {
    const CONTENDED_ATTEMPTS: u64 = 3;
    let started = Instant::now();
    let mut attempts = 0_u64;
    let refresh = || {
        cassie.execute_sql(
            session,
            &format!("REFRESH ROLLUP {DAILY_ROLLUP}"),
            Vec::new(),
        )
    };
    while attempts < CONTENDED_ATTEMPTS {
        attempts = attempts.saturating_add(1);
        match refresh() {
            Ok(_) => return refresh_result(started, attempts, true),
            Err(error) if error.to_string().contains("retry refresh") => {
                shared
                    .refresh_contended_failures
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(error) => panic!("refresh daily time-series rollup: {error}"),
        }
    }
    let quiesce_started = Instant::now();
    let commit_gate = shared.writer_gate.lock().expect("writer gate mutex");
    attempts = attempts.saturating_add(1);
    let outcome = refresh();
    drop(commit_gate);
    shared.refresh_writer_quiesced_millis.store(
        u64::try_from(quiesce_started.elapsed().as_millis()).unwrap_or(u64::MAX),
        Ordering::Release,
    );
    match outcome {
        Ok(_) => refresh_result(started, attempts, true),
        Err(error) => panic!("quiesced rollup refresh failed: {error}"),
    }
}

fn refresh_result(started: Instant, attempts: u64, completed: bool) -> MaintenanceResult {
    MaintenanceResult {
        name: "rollup_refresh".to_owned(),
        elapsed: started.elapsed(),
        completed_batches: attempts,
        completed,
    }
}

pub(super) struct MixedWindow<'a> {
    pub(super) fixture: &'a Fixture,
    pub(super) writer: &'a WriterService,
    pub(super) readers: &'a ReaderPool,
    pub(super) maintenance: &'a MaintenanceWorker,
    pub(super) shared: &'a Arc<SharedMetrics>,
    pub(super) cursor: Option<EventCursor>,
    pub(super) target_rate: u64,
    pub(super) duration: Duration,
    pub(super) start: Instant,
}

pub(super) struct MixedWindowResult {
    pub(super) measured_elapsed: Duration,
    pub(super) maintenance_actions: Vec<MaintenanceAction>,
    pub(super) oracle_page_checks: usize,
    pub(super) oracle_checkpoints: Vec<String>,
    pub(super) oracle_rows_compared: usize,
    pub(super) correction_events_expected: u64,
    pub(super) correction_events_acknowledged: u64,
    pub(super) retention_applied: bool,
    pub(super) backlog_events_at_window_end: u64,
}

fn wait_for_committed_position(shared: &SharedMetrics, target: u64) {
    let deadline = Instant::now() + ORACLE_WAIT_TIMEOUT;
    while shared.committed_position.load(Ordering::Acquire) < target {
        assert!(
            Instant::now() < deadline,
            "checkpoint journal publication deadline"
        );
        thread::yield_now();
    }
}

fn execute_checked(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, Vec::new())
        .unwrap_or_else(|error| panic!("execute campaign SQL {sql:?}: {error}"));
}

pub(super) fn current_fallback_scans(cassie: &Cassie) -> u64 {
    cassie.metrics()["time_series"]["fallback_scans"]
        .as_u64()
        .unwrap_or_default()
}

fn result_event_rows(rows: &[Vec<Value>]) -> Vec<EventRow> {
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

pub(super) struct FinalStateEvidence {
    pub(super) rollup_debt: bool,
    pub(super) fallback_scans: u64,
    pub(super) oracle_rollup_groups_compared: usize,
    pub(super) oracle_rows_compared: usize,
    pub(super) oracle_mismatches: usize,
}

pub(super) fn verify_final_mixed_state(
    fixture: &Fixture,
    shared: &SharedMetrics,
    retention_applied: bool,
    fallback_scans_before: u64,
    rollup_refresh_completed: bool,
) -> FinalStateEvidence {
    let maximum_position = shared.committed_position.load(Ordering::Acquire);
    let committed = shared
        .committed_events
        .lock()
        .expect("committed event log mutex")
        .clone();
    let expected = model::apply_through_checkpoint(
        fixture.initial_rows.as_ref(),
        &committed,
        maximum_position,
        retention_applied,
    );
    let actual = read_relation_in_pages(fixture);
    let expected_relation = expected.values().cloned().collect::<Vec<_>>();
    if actual != expected_relation {
        let first_difference = actual
            .iter()
            .zip(&expected_relation)
            .find(|(left, right)| left != right);
        eprintln!(
            "mixed: final relation mismatch actual_rows={} expected_rows={} first_difference={first_difference:?}",
            actual.len(),
            expected_relation.len()
        );
    }
    let mut mismatches = usize::from(actual != expected_relation);
    let expected_rollups = oracle_rollups(&expected);
    // The plain grouped query is eligible for rollup substitution; the `id >= 0`
    // predicate keeps the same aggregation on the authoritative source rows.
    let grouped = |filter: &str| {
        format!(
            "SELECT time_bucket('1 day',event_at) AS bucket,device,COUNT(*) AS total,SUM(amount) AS amount_sum FROM {TABLE} {filter} GROUP BY time_bucket('1 day',event_at),device ORDER BY bucket,device"
        )
    };
    let rollup_debt = fixture
        .cassie
        .midge
        .has_rollup_maintenance_debt(&fixture.projection)
        .expect("read final rollup debt");
    let source_rollups = read_rollup_map(fixture, &grouped("WHERE id >= 0"));
    if source_rollups != expected_rollups {
        eprintln!("mixed: source rollup mismatch");
        mismatches = mismatches.saturating_add(1);
    }
    if rollup_refresh_completed && !rollup_debt {
        let served_rollups = read_rollup_map(fixture, &grouped(""));
        if served_rollups != expected_rollups {
            let first_difference = served_rollups
                .iter()
                .find(|(key, value)| expected_rollups.get(*key) != Some(*value));
            eprintln!(
                "mixed: rollup-served mismatch first_difference={first_difference:?} expected={:?}",
                first_difference.and_then(|(key, _)| expected_rollups.get(key))
            );
            mismatches = mismatches.saturating_add(1);
        }
    }
    FinalStateEvidence {
        rollup_debt,
        fallback_scans: current_fallback_scans(&fixture.cassie)
            .saturating_sub(fallback_scans_before),
        oracle_rollup_groups_compared: expected_rollups.len(),
        oracle_rows_compared: expected.len(),
        oracle_mismatches: mismatches,
    }
}

/// Reads the whole relation with one ordered scan inside the campaign's query
/// memory budget, so the final comparison does not depend on paging shapes.
fn read_relation_in_pages(fixture: &Fixture) -> Vec<EventRow> {
    let result = fixture
        .execute(&format!(
            "SELECT id,device,event_at,amount FROM {TABLE} ORDER BY id"
        ))
        .unwrap_or_else(|error| {
            panic!(
                "read final relation within the {} byte query budget: {error}",
                crate::workload::fixture::QUERY_MEMORY_BUDGET_BYTES
            )
        });
    result_event_rows(&result)
}

fn oracle_rollups(rows: &BTreeMap<i64, EventRow>) -> BTreeMap<(String, String), (i64, i64)> {
    let mut rollups = BTreeMap::new();
    for row in rows.values() {
        let day = row
            .event_at
            .get(..10)
            .expect("canonical event date")
            .to_owned();
        let aggregate = rollups
            .entry((day, row.device.clone()))
            .or_insert((0_i64, 0_i64));
        aggregate.0 = aggregate.0.saturating_add(1);
        aggregate.1 = aggregate.1.saturating_add(row.amount);
    }
    rollups
}

fn read_rollup_map(fixture: &Fixture, sql: &str) -> BTreeMap<(String, String), (i64, i64)> {
    let result = fixture.execute(sql).expect("read grouped rollup evidence");
    result
        .iter()
        .map(|row| {
            let bucket = value_text(&row[0]);
            let day = bucket.get(..10).expect("rollup bucket date").to_owned();
            let device = row[1].as_str().expect("rollup device is TEXT").to_owned();
            let count = row[2].as_i64().expect("rollup count is BIGINT");
            let amount = row[3].as_i64().expect("rollup sum is BIGINT");
            ((day, device), (count, amount))
        })
        .collect()
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Typed(typed) => typed.text(),
        Value::Int64(number) => number.to_string(),
        other => panic!("unexpected rollup bucket value {other:?}"),
    }
}

pub(super) fn infer_limiting_resource(
    kind: RunKind,
    backlog_at_end: u64,
    rate_attempts: &[RateAttempt],
    reader_p95_us: f64,
    reader_baseline_p95: Option<f64>,
    rollup_debt: bool,
    fallback_scans: u64,
) -> String {
    if let Some(attempt) = rate_attempts.iter().find(|attempt| !attempt.sustainable) {
        if !attempt.drained_within_timeout || attempt.peak_backlog_events > 256 {
            return format!(
                "writer_backlog_capacity_at_{}events_per_second",
                attempt.target_events_per_second
            );
        }
        if attempt.offered_events_per_second < attempt.target_events_per_second {
            return format!(
                "ingest_pacing_ceiling_at_{}events_per_second",
                attempt.target_events_per_second
            );
        }
        return format!(
            "writer_acknowledgement_ceiling_at_{}events_per_second",
            attempt.target_events_per_second
        );
    }
    if let Some(baseline) = reader_baseline_p95 {
        if reader_p95_us > baseline * 2.0 {
            return "mixed_reader_p95_budget".to_owned();
        }
    }
    if matches!(kind, RunKind::Mixed) && rollup_debt {
        return "rollup_maintenance_debt".to_owned();
    }
    if fallback_scans > 0 {
        return "time_series_fallback_scans".to_owned();
    }
    if matches!(kind, RunKind::Mixed) && backlog_at_end > 0 {
        return "writer_backlog_at_window_end".to_owned();
    }
    "no_limit_observed_in_profile".to_owned()
}

pub(super) fn backlog_growth(samples: &[SecondSample]) -> i64 {
    let mut samples = samples.iter().collect::<Vec<_>>();
    samples.sort_by_key(|sample| sample.elapsed_seconds);
    match (samples.first(), samples.last()) {
        (Some(first), Some(last)) => {
            i64::try_from(last.backlog_events).unwrap_or(i64::MAX)
                - i64::try_from(first.backlog_events).unwrap_or(i64::MAX)
        }
        _ => 0,
    }
}

pub(super) fn backlog_is_stable(
    kind: RunKind,
    backlog_at_end: u64,
    samples: &[SecondSample],
) -> bool {
    if !matches!(kind, RunKind::Mixed) {
        return backlog_at_end == 0;
    }
    let tail = samples
        .iter()
        .filter(|sample| {
            (model::scaled_seconds(540)..=model::scaled_seconds(model::MIXED_SECONDS))
                .contains(&sample.elapsed_seconds)
        })
        .collect::<Vec<_>>();
    let (Some(minimum), Some(maximum)) = (
        tail.iter().map(|sample| sample.backlog_events).min(),
        tail.iter().map(|sample| sample.backlog_events).max(),
    ) else {
        return false;
    };
    backlog_at_end <= u64::try_from(model::WRITER_BATCH_EVENTS * 2).unwrap_or(u64::MAX)
        && maximum.saturating_sub(minimum)
            <= u64::try_from(model::WRITER_BATCH_EVENTS * 4).unwrap_or(u64::MAX)
}

pub(super) fn percentile_us(latencies_ns: &[u128], percentile_basis_points: u32) -> f64 {
    if latencies_ns.is_empty() {
        return 0.0;
    }
    let mut sorted = latencies_ns.to_vec();
    sorted.sort_unstable();
    let last_index = u128::try_from(sorted.len() - 1).unwrap_or(u128::MAX);
    let position = last_index
        .saturating_mul(u128::from(percentile_basis_points))
        .saturating_add(9_999)
        / 10_000;
    let index = usize::try_from(position).unwrap_or(sorted.len() - 1);
    let micros = u32::try_from(sorted[index] / 1_000).unwrap_or(u32::MAX);
    f64::from(micros)
}
