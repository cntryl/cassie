//! Cancellation rolls back a source insert and all DO ALSO rule actions (#913).

use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use tokio::io::AsyncWriteExt;

struct RuleFixtureDirectory(String);

impl Drop for RuleFixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn connect_activity_observer(
    address: std::net::SocketAddr,
) -> (
    tokio::net::tcp::OwnedReadHalf,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("activity observer connection");
    let (mut reader, mut writer) = socket.into_split();
    wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    (reader, writer)
}

async fn wait_for_rule_progress_and_pg_sleep(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    target_pid: i32,
    marker: &str,
) -> (bool, bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    let sequence_sql = "SELECT last_value::text, is_called FROM rule_cancel_progress_seq";
    let activity_sql = format!(
        "SELECT state, query, wait_event FROM pg_catalog.pg_stat_activity WHERE pid = {target_pid}"
    );
    let mut progress_observed = false;
    loop {
        wire::write_frames(writer, vec![wire::simple_query_frame(sequence_sql)]).await;
        let progress = wire::read_frames_until_ready_within(reader, Duration::from_secs(2)).await;
        progress_observed |= wire::data_rows(&progress)
            .iter()
            .any(|row| row[0].as_deref() == Some("2") && row[1].as_deref() == Some("t"));

        wire::write_frames(writer, vec![wire::simple_query_frame(&activity_sql)]).await;
        let activity = wire::read_frames_until_ready_within(reader, Duration::from_secs(2)).await;
        let pg_sleep_after_progress = wire::data_rows(&activity).iter().any(|row| {
            row[0].as_deref() == Some("active")
                && row[2].as_deref() == Some("PgSleep")
                && row[1]
                    .as_deref()
                    .is_some_and(|query| query.contains(marker))
        });
        if progress_observed && pg_sleep_after_progress {
            return (true, true);
        }
        if Instant::now() >= deadline {
            return (progress_observed, false);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

type Frames = Vec<(u8, Vec<u8>)>;

struct RuleCancellationSession {
    server: wire::PgwireServer,
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    observer_reader: tokio::net::tcp::OwnedReadHalf,
    observer_writer: tokio::net::tcp::OwnedWriteHalf,
    process_id: i32,
    secret_key: i32,
    setup: Vec<Frames>,
    baseline: Frames,
}

struct RuleCancellationEvidence {
    setup: Vec<Frames>,
    baseline: Frames,
    cancelled: Frames,
    recovery: Vec<Frames>,
    sequence_after: Frames,
    progress_observed: bool,
    pg_sleep_observed: bool,
}

async fn prepare_rule_cancellation_session(path: &str) -> RuleCancellationSession {
    let cassie = Cassie::new_with_data_dir(path).expect("Cassie");
    cassie.startup().expect("start rule cancellation fixture");
    let server = wire::spawn_server(cassie).await;
    let (observer_reader, observer_writer) = connect_activity_observer(server.addr).await;
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("rule action query connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

    let mut setup = Vec::new();
    for sql in [
        "CREATE SEQUENCE rule_cancel_progress_seq",
        "CREATE TABLE rule_cancel_source (id INT4 PRIMARY KEY)",
        "CREATE TABLE rule_cancel_log (id INT4, action_finished BOOL, progress BIGINT)",
        "CREATE RULE delayed_log AS ON INSERT TO rule_cancel_source \
         DO ALSO ( \
             INSERT INTO rule_cancel_log VALUES ( \
                 NEW.id, true, nextval('rule_cancel_progress_seq') \
             ); \
             SELECT pg_sleep_for(INTERVAL '60 seconds') WHERE NEW.id = 2 \
         )",
    ] {
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
        setup.push(wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await);
    }

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(
            "INSERT INTO rule_cancel_source VALUES (1)",
        )],
    )
    .await;
    let baseline = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    RuleCancellationSession {
        server,
        reader,
        writer,
        observer_reader,
        observer_writer,
        process_id,
        secret_key,
        setup,
        baseline,
    }
}

async fn cancel_rule_action_and_read_recovery(
    session: RuleCancellationSession,
) -> RuleCancellationEvidence {
    let RuleCancellationSession {
        server,
        mut reader,
        mut writer,
        mut observer_reader,
        mut observer_writer,
        process_id,
        secret_key,
        setup,
        baseline,
    } = session;

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(
            "INSERT INTO rule_cancel_source VALUES (2) \
             /* issue_913_rule_cancel_after_action */",
        )],
    )
    .await;
    // Require the first DO ALSO action's sequence marker and the exact marked
    // command waiting in the second action's PgSleep before cancellation.
    let (progress_observed, pg_sleep_observed) = wait_for_rule_progress_and_pg_sleep(
        &mut observer_reader,
        &mut observer_writer,
        process_id,
        "issue_913_rule_cancel_after_action",
    )
    .await;
    let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("cancel connection");
    cancel_socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("write rule action cancellation request");
    cancel_socket
        .shutdown()
        .await
        .expect("close rule action cancellation request");
    let cancelled = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    let mut recovery = Vec::new();
    for sql in [
        "SELECT id FROM rule_cancel_source ORDER BY id",
        "SELECT id, action_finished, progress FROM rule_cancel_log ORDER BY id",
    ] {
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
        recovery
            .push(wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await);
    }
    wire::write_frames(
        &mut observer_writer,
        vec![wire::simple_query_frame(
            "SELECT last_value::text, is_called FROM rule_cancel_progress_seq",
        )],
    )
    .await;
    let sequence_after =
        wire::read_frames_until_ready_within(&mut observer_reader, Duration::from_secs(5)).await;

    drop(reader);
    drop(writer);
    drop(observer_reader);
    drop(observer_writer);
    server.stop().await;
    RuleCancellationEvidence {
        setup,
        baseline,
        cancelled,
        recovery,
        sequence_after,
        progress_observed,
        pg_sleep_observed,
    }
}

fn assert_rule_cancellation(evidence: &RuleCancellationEvidence) {
    for (index, frames) in evidence.setup.iter().enumerate() {
        assert_eq!(
            wire::error_code(frames),
            None,
            "rule action setup step {index} ErrorResponse: {:?}",
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(
        wire::error_code(&evidence.baseline),
        None,
        "finite rule action control ErrorResponse: {:?}",
        evidence
            .baseline
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&evidence.baseline),
        Vec::<Vec<Option<String>>>::new(),
        "the finite second rule action returns no rows for source id 1"
    );
    assert!(
        evidence.progress_observed,
        "the DO ALSO action must advance its sequence before cancellation"
    );
    assert!(
        evidence.pg_sleep_observed,
        "the marked source INSERT must reach PgSleep after its DO ALSO progress marker"
    );
    assert_eq!(
        wire::error_code(&evidence.cancelled).as_deref(),
        Some("57014"),
        "cancelled rule action ErrorResponse: {:?}",
        evidence
            .cancelled
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&evidence.cancelled),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        evidence
            .cancelled
            .iter()
            .filter(|frame| frame.0 == b'C')
            .count(),
        0
    );
    assert_eq!(evidence.cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire::error_code(&evidence.recovery[0]), None);
    assert_eq!(
        wire::data_rows(&evidence.recovery[0]),
        vec![vec![Some("1".to_owned())]],
        "cancelled source row must roll back"
    );
    assert_eq!(wire::error_code(&evidence.recovery[1]), None);
    assert_eq!(
        wire::data_rows(&evidence.recovery[1]),
        vec![vec![
            Some("1".to_owned()),
            Some("t".to_owned()),
            Some("1".to_owned()),
        ]],
        "cancelled DO ALSO action must roll back with its source insert"
    );
    assert_eq!(wire::error_code(&evidence.sequence_after), None);
    assert_eq!(
        wire::data_rows(&evidence.sequence_after),
        vec![vec![Some("2".to_owned()), Some("t".to_owned())]],
        "the sequence records rule-action progress even though the transaction rolls back"
    );
}

#[test]
fn should_cancel_without_partial_mutations_given_running_rule_actions_when_cancel_request_arrives()
{
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("rule-actions-cancellation-atomicity");
    let _directory = RuleFixtureDirectory(path.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let evidence = runtime.block_on(async {
        let session = prepare_rule_cancellation_session(&path).await;
        cancel_rule_action_and_read_recovery(session).await
    });
    drop(runtime);

    // Assert
    assert_rule_cancellation(&evidence);
}
