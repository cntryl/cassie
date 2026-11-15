//! Actual child-process death at the replay commit and acknowledgment boundaries.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

use crate::app::{Cassie, CassieError, ProjectionReplayBatch, ProjectionReplayEvent};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_schema_write_commit_barriers, ScalarIndexScanRequest, SchemaWritePausePoint,
};

const CHILD_CASE: &str = "CASSIE_TEST_REPLAY_PROCESS_DEATH_CASE";
const CHILD_PATH: &str = "CASSIE_TEST_REPLAY_PROCESS_DEATH_PATH";
const CHILD_TOKEN: &str = "CASSIE_TEST_REPLAY_PROCESS_DEATH_TOKEN";
const OUTPUT_LIMIT: u64 = 256 * 1024;
const CHILD_DEADLINE: Duration = Duration::from_secs(30);
// Midge 0.5.2 uses a 30-second local lease and a default half-TTL skew allowance.
// This larger deadline is a hang guard, not a promised takeover duration.
const RECOVERY_DEADLINE: Duration = Duration::from_secs(90);
const LEASE_RECHECK_INTERVAL: Duration = Duration::from_millis(250);
const BEFORE_TEST: &str = "midge::adapter::statement_read::tests::replay_process_crash::should_recover_previous_replay_after_process_death_before_commit";
const AFTER_TEST: &str = "midge::adapter::statement_read::tests::replay_process_crash::should_recover_acknowledged_replay_after_process_death";
const BUFFERED_BEFORE_TEST: &str = "midge::adapter::statement_read::tests::replay_process_crash::should_recover_prior_state_after_buffered_replay_process_death_before_commit";
const BUFFERED_AFTER_TEST: &str = "midge::adapter::statement_read::tests::replay_process_crash::should_reconcile_buffered_replay_after_acknowledged_process_death";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    BeforeCommit,
    AfterAcknowledgment,
    BufferedBeforeCommit,
    BufferedAfterAcknowledgment,
}

impl Boundary {
    const fn label(self) -> &'static str {
        match self {
            Self::BeforeCommit => "before_commit",
            Self::AfterAcknowledgment => "after_acknowledgment",
            Self::BufferedBeforeCommit => "buffered_before_commit",
            Self::BufferedAfterAcknowledgment => "buffered_after_acknowledgment",
        }
    }

    const fn replayable(self) -> bool {
        matches!(
            self,
            Self::BufferedBeforeCommit | Self::BufferedAfterAcknowledgment
        )
    }

    const fn test_name(self) -> &'static str {
        match self {
            Self::BeforeCommit => BEFORE_TEST,
            Self::AfterAcknowledgment => AFTER_TEST,
            Self::BufferedBeforeCommit => BUFFERED_BEFORE_TEST,
            Self::BufferedAfterAcknowledgment => BUFFERED_AFTER_TEST,
        }
    }
}

struct OwnedDataDir(PathBuf);

impl OwnedDataDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-replay-process-death-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).expect("create owned replay process-death directory");
        Self(path)
    }
}

impl Drop for OwnedDataDir {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).expect("remove owned replay process-death directory");
        }
    }
}

struct OwnedChild(Child);

impl OwnedChild {
    fn kill_and_reap(&mut self) -> (bool, ExitStatus) {
        let status = self.0.try_wait().expect("inspect child status");
        if let Some(status) = status {
            return (false, status);
        }
        self.0.kill().expect("kill replay child");
        (true, self.0.wait().expect("reap replay child"))
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn local_cassie(path: &Path) -> Cassie {
    let cassie = Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default())
        .expect("open explicit local Midge storage");
    cassie.startup().expect("start local Cassie");
    cassie
}

fn is_writer_lease_held(error: &CassieError) -> bool {
    matches!(error, CassieError::Storage(message) if message.starts_with("Writer lease held: "))
}

fn recover_after_writer_lease(path: &Path) -> Cassie {
    let deadline = Instant::now() + RECOVERY_DEADLINE;
    loop {
        match Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default()) {
            Ok(cassie) => {
                cassie.startup().expect("start recovered local Cassie");
                return cassie;
            }
            Err(error) if is_writer_lease_held(&error) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                assert!(!remaining.is_zero(), "writer lease did not expire: {error}");
                // Wait only while the actual lease is held; an early unpark
                // simply rechecks it. No lease record is changed by this test.
                std::thread::park_timeout(LEASE_RECHECK_INTERVAL.min(remaining));
            }
            Err(error) => panic!("unexpected local recovery error: {error}"),
        }
    }
}

fn replay_batch(projection: &str, position: u64, title: &str) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_owned(),
        source_identity: "orders".to_owned(),
        batch_id: format!("batch-{position}"),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: format!("event-{position}"),
            checkpoint: format!("checkpoint-{position}"),
            position: Some(position),
            document_id: "doc-1".to_owned(),
            payload: Some(serde_json::json!({"title": title})),
        }],
    }
}

fn create_baseline(path: &Path) -> String {
    let cassie = local_cassie(path);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE replay_process_death (title TEXT)",
            vec![],
        )
        .expect("create projection table");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX replay_process_death_title_idx ON replay_process_death USING btree (title)",
            vec![],
        )
        .expect("create projection index");
    let projection = cassie
        .midge
        .canonical_collection_name("replay_process_death");
    let first = cassie
        .replay_projection_batch_strict(replay_batch(&projection, 1, "before"))
        .expect("commit initial projection");
    assert_eq!(first.applied_event_count, 1);
    cassie.shutdown();
    drop(cassie);
    projection
}

fn index_ids(cassie: &Cassie, projection: &str, title: &str) -> Vec<String> {
    let index = cassie
        .midge
        .get_index(projection, "replay_process_death_title_idx")
        .expect("read index metadata")
        .expect("index exists");
    let controls = cassie.runtime.query_controls(Instant::now());
    let request = ScalarIndexScanRequest {
        equality_prefix: vec![serde_json::json!(title)],
        ..Default::default()
    };
    cassie
        .midge
        .scan_scalar_index_controlled(&index, &request, &controls)
        .expect("scan persisted scalar index")
        .as_slice()
        .iter()
        .map(|hit| hit.id.clone())
        .collect()
}

fn assert_recovered_state(cassie: &Cassie, projection: &str, expected_position: u64) {
    let expected_title = if expected_position == 1 {
        "before"
    } else {
        "after"
    };
    let other_title = if expected_position == 1 {
        "after"
    } else {
        "before"
    };
    let row = cassie
        .midge
        .get_document(projection, "doc-1")
        .expect("read recovered row")
        .expect("recovered row exists");
    let metadata = cassie
        .midge
        .projection_metadata(projection)
        .expect("read recovered checkpoint")
        .expect("checkpoint metadata exists");
    assert_eq!(row.payload, serde_json::json!({"title": expected_title}));
    assert_eq!(
        metadata.source_checkpoint,
        Some(format!("checkpoint-{expected_position}"))
    );
    assert_eq!(metadata.source_position, Some(expected_position));
    assert_eq!(
        metadata.last_applied_event_id,
        Some(format!("event-{expected_position}"))
    );
    assert!(cassie
        .midge
        .has_projection_event(projection, "orders", "event-1")
        .unwrap());
    assert_eq!(
        cassie
            .midge
            .has_projection_event(projection, "orders", "event-2")
            .unwrap(),
        expected_position == 2
    );
    assert_eq!(
        index_ids(cassie, projection, expected_title),
        vec!["doc-1".to_owned()]
    );
    assert_eq!(
        index_ids(cassie, projection, other_title),
        Vec::<String>::new()
    );
}

fn emit_marker_and_wait(boundary: Boundary) -> ! {
    let token = std::env::var(CHILD_TOKEN).expect("child token");
    let mut stdout = std::io::stdout().lock();
    writeln!(
        stdout,
        "CASSIE_REPLAY_PROCESS_DEATH|{}|{token}|READY",
        boundary.label()
    )
    .expect("write framed child readiness");
    stdout.flush().expect("flush child readiness");
    drop(stdout);
    let mut byte = [0_u8; 1];
    std::io::stdin()
        .read_exact(&mut byte)
        .expect("parent should terminate child while paused");
    panic!("parent unexpectedly resumed the replay child");
}

fn run_replay(
    cassie: &Cassie,
    batch: ProjectionReplayBatch,
    replayable: bool,
) -> Result<crate::app::ProjectionReplayReport, CassieError> {
    if replayable {
        cassie.replay_projection_batch(batch)
    } else {
        cassie.replay_projection_batch_strict(batch)
    }
}

fn child_main(boundary: Boundary) -> ! {
    assert_eq!(std::env::var(CHILD_CASE).as_deref(), Ok(boundary.label()));
    let path = PathBuf::from(std::env::var_os(CHILD_PATH).expect("child storage path"));
    let cassie = Arc::new(local_cassie(&path));
    let projection = cassie
        .midge
        .canonical_collection_name("replay_process_death");
    let batch = replay_batch(&projection, 2, "after");
    match boundary {
        Boundary::BeforeCommit | Boundary::BufferedBeforeCommit => {
            let _guard = schema_write_conflict_test_guard();
            let ready = Arc::new(Barrier::new(2));
            let resume = Arc::new(Barrier::new(2));
            set_schema_write_commit_barriers(
                Some(SchemaWritePausePoint::ProjectionReplayMetadataCommit),
                Some(Arc::clone(&ready)),
                Some(resume),
            );
            let writer = Arc::clone(&cassie);
            let _writer = std::thread::spawn(move || {
                let _worker = schema_write_conflict_worker_guard();
                run_replay(&writer, batch, boundary.replayable())
                    .expect("replay should remain paused before commit");
            });
            ready.wait();
            emit_marker_and_wait(boundary);
        }
        Boundary::AfterAcknowledgment | Boundary::BufferedAfterAcknowledgment => {
            let report =
                run_replay(&cassie, batch, boundary.replayable()).expect("acknowledge replay");
            assert_eq!(report.applied_event_count, 1);
            emit_marker_and_wait(boundary);
        }
    }
}

fn kill_at_boundary(path: &Path, boundary: Boundary) {
    let token = uuid::Uuid::new_v4().to_string();
    let marker = format!(
        "CASSIE_REPLAY_PROCESS_DEATH|{}|{token}|READY",
        boundary.label()
    );
    let child = Command::new(std::env::current_exe().expect("current test executable"))
        .args([
            "--exact",
            boundary.test_name(),
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_CASE, boundary.label())
        .env(CHILD_PATH, path)
        .env(CHILD_TOKEN, &token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn isolated replay child");
    let mut child = OwnedChild(child);
    let output = child.0.stdout.take().expect("child stdout");
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let found = BufReader::new(output.take(OUTPUT_LIMIT))
            .lines()
            .any(|line| line.is_ok_and(|line| line.ends_with(&marker)));
        let _ = sender.send(found);
    });
    let marker_seen = receiver.recv_timeout(CHILD_DEADLINE);
    // This is unconditional, including timeout and EOF. Reopen only after wait().
    let (was_running, status) = child.kill_and_reap();
    reader.join().expect("join bounded child stdout reader");
    assert_eq!(
        marker_seen,
        Ok(true),
        "selected replay child did not reach {boundary:?}"
    );
    assert!(
        was_running,
        "replay child exited before the parent killed it: {status}"
    );
    assert!(
        !status.success(),
        "replay child unexpectedly exited successfully"
    );
}

fn prepare_process_death(boundary: Boundary) -> (OwnedDataDir, String) {
    if std::env::var_os(CHILD_CASE).is_some() {
        child_main(boundary);
    }
    let directory = OwnedDataDir::new();
    let projection = create_baseline(&directory.0);
    (directory, projection)
}

fn assert_retry_reconciles(
    recovered: Cassie,
    projection: &str,
    expected_position: u64,
    replayable: bool,
) {
    assert_recovered_state(&recovered, projection, expected_position);
    let second = replay_batch(projection, 2, "after");
    let retry =
        run_replay(&recovered, second.clone(), replayable).expect("reconcile recovered checkpoint");
    assert_eq!(retry.applied_event_count, u64::from(expected_position == 1));
    assert_eq!(
        retry.skipped_duplicate_count,
        u64::from(expected_position == 2)
    );
    assert_recovered_state(&recovered, projection, 2);
    let duplicate = run_replay(&recovered, second, replayable).expect("retry same batch again");
    assert_eq!(duplicate.applied_event_count, 0);
    assert_eq!(duplicate.skipped_duplicate_count, 1);
    assert_recovered_state(&recovered, projection, 2);
    recovered.shutdown();
    drop(recovered);
}

#[test]
fn should_recover_previous_replay_after_process_death_before_commit() {
    // Arrange
    let (directory, projection) = prepare_process_death(Boundary::BeforeCommit);

    // Act
    kill_at_boundary(&directory.0, Boundary::BeforeCommit);
    let recovered = recover_after_writer_lease(&directory.0);

    // Assert
    assert_retry_reconciles(recovered, &projection, 1, false);
}

#[test]
fn should_recover_acknowledged_replay_after_process_death() {
    // Arrange
    let (directory, projection) = prepare_process_death(Boundary::AfterAcknowledgment);

    // Act
    kill_at_boundary(&directory.0, Boundary::AfterAcknowledgment);
    let recovered = recover_after_writer_lease(&directory.0);

    // Assert
    assert_retry_reconciles(recovered, &projection, 2, false);
}

#[test]
fn should_retry_only_the_writer_lease_held_error_during_recovery() {
    // Arrange
    let errors = [
        crate::app::CassieError::from(cntryl_midge::MidgeError::LeaseHeld(
            "other writer".to_owned(),
        )),
        crate::app::CassieError::Storage("another storage failure".to_owned()),
        crate::app::CassieError::Storage("Writer lease heldish: wrong prefix".to_owned()),
        crate::app::CassieError::StorageBootstrap("Writer lease held: wrong variant".to_owned()),
    ];

    // Act
    let retry = errors.each_ref().map(is_writer_lease_held);

    // Assert
    assert_eq!(retry, [true, false, false, false]);
}

#[test]
fn should_recover_prior_state_after_buffered_replay_process_death_before_commit() {
    // Arrange
    let (directory, projection) = prepare_process_death(Boundary::BufferedBeforeCommit);

    // Act
    kill_at_boundary(&directory.0, Boundary::BufferedBeforeCommit);
    let recovered = recover_after_writer_lease(&directory.0);

    // Assert
    assert_retry_reconciles(recovered, &projection, 1, true);
}

#[test]
fn should_reconcile_buffered_replay_after_acknowledged_process_death() {
    // Arrange
    let (directory, projection) = prepare_process_death(Boundary::BufferedAfterAcknowledgment);

    // Act
    kill_at_boundary(&directory.0, Boundary::BufferedAfterAcknowledgment);
    let recovered = recover_after_writer_lease(&directory.0);
    let position = recovered
        .midge
        .projection_metadata(&projection)
        .unwrap()
        .unwrap()
        .source_position
        .unwrap();

    // Assert
    assert!(
        position == 1 || position == 2,
        "unexpected recovered position {position}"
    );
    assert_retry_reconciles(recovered, &projection, position, true);
}
