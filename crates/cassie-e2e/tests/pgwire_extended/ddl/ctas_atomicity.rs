//! Internal CTAS creation stays inside its enclosing wire transaction.

use crate::support_pgwire as wire;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

type Frames = Vec<(u8, Vec<u8>)>;

async fn simple_query(
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncRead + Unpin),
    sql: &str,
) -> Frames {
    wire::write_frames(writer, vec![wire::simple_query_frame(sql)]).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

async fn prepare_extended(
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncRead + Unpin),
    name: &str,
    sql: &str,
) -> Frames {
    wire::write_frames(
        writer,
        vec![
            wire::parse_frame(name, sql),
            wire::bind_frame(name, name, &[]),
            wire::sync_frame(),
        ],
    )
    .await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

async fn execute_extended(
    writer: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncRead + Unpin),
    portal: &str,
) -> Frames {
    wire::write_frames(
        writer,
        vec![wire::execute_frame(portal), wire::sync_frame()],
    )
    .await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

async fn cancel_backend(address: std::net::SocketAddr, process_id: i32, secret_key: i32) {
    let mut socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect CTAS CancelRequest socket");
    socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("send CTAS CancelRequest");
    socket
        .shutdown()
        .await
        .expect("close CTAS CancelRequest socket");
    let mut response = [0; 1];
    let received = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut response))
        .await
        .expect("CancelRequest should be handled")
        .expect("read CancelRequest close");
    assert_eq!(received, 0, "CancelRequest has no response body");
}

fn ready_status(frames: &Frames) -> Option<u8> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'Z')
        .and_then(|(_, body)| body.first().copied())
}

fn ctas_cycles(succeed: bool) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let server = wire::spawn_server(cassie).await;
        for extended in [false, true] {
            let connection = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connection");
            let (mut reader, mut writer) = connection.into_split();
            wire::complete_startup(&mut reader, &mut writer).await;
            let name = format!("wire_ctas_{extended}_{succeed}");
            let expression = if succeed { "1" } else { "1 / 0" };
            let sql = format!("CREATE TABLE {name} AS SELECT {expression} AS n");
            let frames = if extended {
                vec![
                    wire::parse_frame("ctas", &sql),
                    wire::bind_frame("", "ctas", &[]),
                    wire::execute_frame(""),
                    wire::sync_frame(),
                ]
            } else {
                vec![wire::simple_query_frame(&sql)]
            };

            // Act
            wire::write_frames(&mut writer, frames).await;
            let result =
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
            let inspect = if succeed {
                format!("SELECT n FROM {name}")
            } else {
                format!("SELECT count(*) FROM pg_class WHERE relname = '{name}'")
            };
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(&inspect)]).await;
            let observed =
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;

            // Assert
            assert_eq!(
                result.last().map(|(_, body)| body.as_slice()),
                Some(&b"I"[..])
            );
            if succeed {
                assert_eq!(wire::error_code(&result), None);
                assert_eq!(
                    result
                        .iter()
                        .find(|(tag, _)| *tag == b'C')
                        .map(|(_, body)| body.as_slice()),
                    Some(&b"SELECT 1\0"[..])
                );
            } else {
                assert_eq!(wire::error_code(&result).as_deref(), Some("22012"));
                assert!(result.iter().all(|(tag, _)| *tag != b'C'));
            }
            assert_eq!(wire::error_code(&observed), None);
            let expected = if succeed { "1" } else { "0" };
            assert_eq!(
                wire::data_rows(&observed),
                vec![vec![Some(expected.to_owned())]]
            );
        }
        server.stop().await;
    });
}

#[test]
fn should_rollback_failed_ctas_in_both_wire_protocols() {
    // Arrange
    let succeed = false;
    // Act
    ctas_cycles(succeed);
    // Assert
    // Each actual protocol must return 22012, ReadyForQuery(I) and no relation.
}

#[test]
fn should_publish_successful_ctas_in_both_wire_protocols() {
    // Arrange
    let succeed = true;
    // Act
    ctas_cycles(succeed);
    // Assert
    // Each protocol must expose exactly the committed row after completion.
}

struct CtasCancellationSession {
    server: wire::PgwireServer,
    owner_reader: tokio::net::tcp::OwnedReadHalf,
    owner_writer: tokio::net::tcp::OwnedWriteHalf,
    observer_reader: tokio::net::tcp::OwnedReadHalf,
    observer_writer: tokio::net::tcp::OwnedWriteHalf,
    process_id: i32,
    secret_key: i32,
}

struct CtasCancellationEvidence {
    begun: Frames,
    savepoint: Frames,
    prepared: Frames,
    observer_during: Frames,
    canceled: Frames,
    observer_after: Frames,
    recovered: Frames,
    owner_after_rollback: Frames,
    retry_prepared: Frames,
    retried: Frames,
    committed: Frames,
    visible: Frames,
}

struct CtasRecoveryEvidence {
    recovered: Frames,
    owner_after_rollback: Frames,
    retry_prepared: Frames,
    retried: Frames,
    committed: Frames,
    visible: Frames,
}

async fn start_ctas_cancellation_session() -> CtasCancellationSession {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
    cassie.startup().expect("startup");
    let server = wire::spawn_server(cassie).await;
    let owner_socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("CTAS owner connection");
    let (mut owner_reader, mut owner_writer) = owner_socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut owner_reader, &mut owner_writer).await;
    let observer_socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("CTAS observer connection");
    let (mut observer_reader, mut observer_writer) = observer_socket.into_split();
    wire::complete_startup(&mut observer_reader, &mut observer_writer).await;
    CtasCancellationSession {
        server,
        owner_reader,
        owner_writer,
        observer_reader,
        observer_writer,
        process_id,
        secret_key,
    }
}

async fn prepare_ctas_cancellation(
    session: &mut CtasCancellationSession,
) -> (Frames, Frames, Frames) {
    let begun = simple_query(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "BEGIN",
    )
    .await;
    let savepoint = simple_query(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "SAVEPOINT before_ctas",
    )
    .await;
    let sql = "CREATE TABLE wire_issue833_ctas_canceled AS \
               SELECT 1 AS n WHERE pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL";
    let prepared = prepare_extended(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "canceled_ctas",
        sql,
    )
    .await;
    (begun, savepoint, prepared)
}

async fn cancel_staged_ctas(session: &mut CtasCancellationSession) -> (Frames, Frames, Frames) {
    let (_pause_guard, selected, resume) = cassie::runtime::pause_next_query_at_sleep_entry();
    wire::write_frames(
        &mut session.owner_writer,
        vec![wire::execute_frame("canceled_ctas"), wire::sync_frame()],
    )
    .await;
    tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
        .await
        .expect("sleep-entry observer task")
        .expect("CTAS source must enter sleep after staging its target relation");
    let observer_during = simple_query(
        &mut session.observer_writer,
        &mut session.observer_reader,
        "SELECT count(*) FROM pg_class WHERE relname = 'wire_issue833_ctas_canceled'",
    )
    .await;
    cancel_backend(session.server.addr, session.process_id, session.secret_key).await;
    resume.send(()).expect("resume CTAS after cancellation");
    let canceled =
        wire::read_frames_until_ready_within(&mut session.owner_reader, Duration::from_secs(5))
            .await;
    let observer_after = simple_query(
        &mut session.observer_writer,
        &mut session.observer_reader,
        "SELECT count(*) FROM pg_class WHERE relname = 'wire_issue833_ctas_canceled'",
    )
    .await;
    (observer_during, canceled, observer_after)
}

async fn recover_and_publish_ctas(session: &mut CtasCancellationSession) -> CtasRecoveryEvidence {
    let recovered = simple_query(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "ROLLBACK TO SAVEPOINT before_ctas",
    )
    .await;
    let owner_after_rollback = simple_query(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "SELECT count(*) FROM pg_class WHERE relname = 'wire_issue833_ctas_canceled'",
    )
    .await;
    let retry_prepared = prepare_extended(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "committed_ctas",
        "CREATE TABLE wire_issue833_ctas_canceled AS SELECT 7 AS n",
    )
    .await;
    let retried = execute_extended(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "committed_ctas",
    )
    .await;
    let committed = simple_query(
        &mut session.owner_writer,
        &mut session.owner_reader,
        "COMMIT",
    )
    .await;
    let visible = simple_query(
        &mut session.observer_writer,
        &mut session.observer_reader,
        "SELECT n FROM wire_issue833_ctas_canceled",
    )
    .await;
    CtasRecoveryEvidence {
        recovered,
        owner_after_rollback,
        retry_prepared,
        retried,
        committed,
        visible,
    }
}

async fn finish_ctas_cancellation_session(session: CtasCancellationSession) {
    let CtasCancellationSession {
        server,
        owner_reader,
        owner_writer,
        observer_reader,
        observer_writer,
        ..
    } = session;
    drop(owner_reader);
    drop(owner_writer);
    drop(observer_reader);
    drop(observer_writer);
    server.stop().await;
}

#[test]
fn should_recover_to_savepoint_after_canceled_extended_ctas() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    // Act
    let evidence = runtime.block_on(async {
        let mut session = start_ctas_cancellation_session().await;
        let (begun, savepoint, prepared) = prepare_ctas_cancellation(&mut session).await;
        let (observer_during, canceled, observer_after) = cancel_staged_ctas(&mut session).await;
        let recovery = recover_and_publish_ctas(&mut session).await;
        finish_ctas_cancellation_session(session).await;
        CtasCancellationEvidence {
            begun,
            savepoint,
            prepared,
            observer_during,
            canceled,
            observer_after,
            recovered: recovery.recovered,
            owner_after_rollback: recovery.owner_after_rollback,
            retry_prepared: recovery.retry_prepared,
            retried: recovery.retried,
            committed: recovery.committed,
            visible: recovery.visible,
        }
    });

    // Assert
    assert_ctas_cancellation_evidence(&evidence);
}

fn assert_ctas_cancellation_evidence(evidence: &CtasCancellationEvidence) {
    assert_eq!(wire::error_code(&evidence.begun), None);
    assert_eq!(ready_status(&evidence.begun), Some(b'T'));
    assert_eq!(wire::error_code(&evidence.savepoint), None);
    assert_eq!(ready_status(&evidence.savepoint), Some(b'T'));
    assert_eq!(wire::error_code(&evidence.prepared), None);
    assert_eq!(ready_status(&evidence.prepared), Some(b'T'));
    assert_eq!(
        wire::data_rows(&evidence.observer_during),
        vec![vec![Some("0".into())]]
    );
    assert_eq!(
        wire::error_code(&evidence.canceled).as_deref(),
        Some("57014")
    );
    assert_eq!(ready_status(&evidence.canceled), Some(b'E'));
    assert!(!evidence.canceled.iter().any(|(tag, _)| *tag == b'C'));
    assert_eq!(
        wire::data_rows(&evidence.observer_after),
        vec![vec![Some("0".into())]]
    );
    assert_eq!(wire::error_code(&evidence.recovered), None);
    assert_eq!(ready_status(&evidence.recovered), Some(b'T'));
    assert_eq!(
        wire::data_rows(&evidence.owner_after_rollback),
        vec![vec![Some("0".into())]]
    );
    assert_eq!(wire::error_code(&evidence.retry_prepared), None);
    assert_eq!(ready_status(&evidence.retry_prepared), Some(b'T'));
    assert_eq!(wire::error_code(&evidence.retried), None);
    assert_eq!(ready_status(&evidence.retried), Some(b'T'));
    assert_eq!(
        evidence
            .retried
            .iter()
            .find(|(tag, _)| *tag == b'C')
            .map(|(_, body)| body.as_slice()),
        Some(&b"SELECT 1\0"[..])
    );
    assert_eq!(wire::error_code(&evidence.committed), None);
    assert_eq!(ready_status(&evidence.committed), Some(b'I'));
    assert_eq!(wire::error_code(&evidence.visible), None);
    assert_eq!(
        wire::data_rows(&evidence.visible),
        vec![vec![Some("7".into())]]
    );
}
