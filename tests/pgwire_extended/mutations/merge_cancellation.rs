//! MERGE cancellation leaves no partial RETURNING rows or target mutations (#823).

use std::time::Duration;

use crate::support_pgwire as wire;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

type Frames = Vec<(u8, Vec<u8>)>;

struct MergeSession {
    server: wire::PgwireServer,
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    observer_reader: tokio::net::tcp::OwnedReadHalf,
    observer_writer: tokio::net::tcp::OwnedWriteHalf,
    process_id: i32,
    secret_key: i32,
    setup: Vec<Frames>,
    baseline: Frames,
    source_update: Frames,
}

struct MergeCancellationEvidence {
    setup: Vec<Frames>,
    baseline: Frames,
    source_update: Frames,
    cancelled: Frames,
    recovery: Frames,
    progress_observed: bool,
}

async fn prepare_merge_session() -> MergeSession {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("start MERGE cancellation fixture");
    let server = wire::spawn_server(cassie).await;
    let (observer_reader, observer_writer) = connect_activity_observer(server.addr).await;
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("MERGE query connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

    let mut setup = Vec::new();
    for sql in [
        "CREATE SEQUENCE merge_cancel_progress_seq",
        "CREATE TABLE merge_cancel_target (id INT4 PRIMARY KEY, value INT4)",
        "CREATE TABLE merge_cancel_source (id INT4, value INT4)",
        "ALTER TABLE merge_cancel_target ADD COLUMN progress BIGINT",
        "INSERT INTO merge_cancel_target VALUES (1, 10, 0)",
        "INSERT INTO merge_cancel_source VALUES (1, 20)",
    ] {
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(sql)]).await;
        setup.push(wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await);
    }

    let baseline_sql = "MERGE INTO merge_cancel_target AS target \
        USING merge_cancel_source AS source ON target.id = source.id \
        WHEN MATCHED THEN UPDATE SET value = source.value, \
            progress = nextval('merge_cancel_progress_seq') \
        RETURNING target.id, target.value, \
            pg_sleep_for(INTERVAL '0 seconds') IS NOT NULL AS wait_complete";
    wire::write_frames(&mut writer, vec![wire::simple_query_frame(baseline_sql)]).await;
    let baseline = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(
            "UPDATE merge_cancel_source SET value = 30 WHERE id = 1",
        )],
    )
    .await;
    let source_update =
        wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    MergeSession {
        server,
        reader,
        writer,
        observer_reader,
        observer_writer,
        process_id,
        secret_key,
        setup,
        baseline,
        source_update,
    }
}

async fn cancel_merge_and_read_recovery(session: MergeSession) -> MergeCancellationEvidence {
    let MergeSession {
        server,
        mut reader,
        mut writer,
        mut observer_reader,
        mut observer_writer,
        process_id,
        secret_key,
        setup,
        baseline,
        source_update,
    } = session;

    let sleeping_merge = "MERGE INTO merge_cancel_target AS target \
        USING merge_cancel_source AS source ON target.id = source.id \
        WHEN MATCHED THEN UPDATE SET value = source.value, \
            progress = nextval('merge_cancel_progress_seq') \
        RETURNING target.id, target.value, \
            pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL AS wait_complete \
        /* issue_823_merge_cancel_after_update */";
    let (_pause_guard, selected, resume) = cassie::runtime::pause_next_query_at_sleep_entry();
    wire::write_frames(&mut writer, vec![wire::simple_query_frame(sleeping_merge)]).await;
    tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
        .await
        .expect("sleep-entry observer task")
        .expect("MERGE must reach RETURNING sleep after its matched UPDATE");

    // The sleep-entry barrier makes this read deterministic: the matched
    // UPDATE has evaluated nextval before RETURNING enters the paused sleep.
    wire::write_frames(
        &mut observer_writer,
        vec![wire::simple_query_frame(
            "SELECT last_value::text FROM pg_catalog.pg_sequences \
             WHERE sequencename = 'merge_cancel_progress_seq'",
        )],
    )
    .await;
    let progress =
        wire::read_frames_until_ready_within(&mut observer_reader, Duration::from_secs(5)).await;
    let progress_observed = wire::error_code(&progress).is_none()
        && wire::data_rows(&progress)
            .iter()
            .any(|row| row[0].as_deref() == Some("2"));

    let mut cancel_socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("cancel connection");
    cancel_socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("write MERGE cancellation request");
    cancel_socket
        .shutdown()
        .await
        .expect("close MERGE cancellation request");
    let mut cancel_response = [0; 1];
    let cancel_response_bytes = cancel_socket
        .read(&mut cancel_response)
        .await
        .expect("wait for cancellation connection to close");
    assert_eq!(
        cancel_response_bytes, 0,
        "CancelRequest has no response body"
    );
    resume
        .send(())
        .expect("resume paused MERGE after CancelRequest");
    let cancelled = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(
            "SELECT id, value, progress FROM merge_cancel_target ORDER BY id",
        )],
    )
    .await;
    let recovery = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    drop(reader);
    drop(writer);
    drop(observer_reader);
    drop(observer_writer);
    server.stop().await;
    MergeCancellationEvidence {
        setup,
        baseline,
        source_update,
        cancelled,
        recovery,
        progress_observed,
    }
}

fn assert_merge_cancellation(evidence: &MergeCancellationEvidence) {
    for (index, frames) in evidence.setup.iter().enumerate() {
        assert_eq!(
            wire::error_code(frames),
            None,
            "MERGE setup step {index} ErrorResponse: {:?}",
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
        "finite MERGE control ErrorResponse: {:?}",
        evidence
            .baseline
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&evidence.baseline),
        vec![vec![
            Some("1".to_owned()),
            Some("20".to_owned()),
            Some("t".to_owned()),
        ]]
    );
    assert_eq!(wire::error_code(&evidence.source_update), None);
    assert!(
        evidence.progress_observed,
        "MERGE must evaluate its matched UPDATE before RETURNING sleep is entered"
    );
    assert_eq!(
        wire::error_code(&evidence.cancelled).as_deref(),
        Some("57014"),
        "cancelled MERGE ErrorResponse: {:?}",
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
    assert_eq!(wire::error_code(&evidence.recovery), None);
    assert_eq!(
        wire::data_rows(&evidence.recovery),
        vec![vec![
            Some("1".to_owned()),
            Some("20".to_owned()),
            Some("1".to_owned()),
        ]],
        "the cancelled MERGE must roll its matched UPDATE back after progress value 2"
    );
}

#[test]
fn should_cancel_without_partial_mutations_or_returning_rows_given_a_running_merge_when_cancel_request_arrives(
) {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let evidence = runtime.block_on(async {
        let session = prepare_merge_session().await;
        cancel_merge_and_read_recovery(session).await
    });
    drop(runtime);

    // Assert
    assert_merge_cancellation(&evidence);
}
