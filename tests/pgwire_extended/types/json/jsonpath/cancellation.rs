//! Cancellation while `JSONPath` rows feed an aggregate over pgwire.

use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture_with_config;
use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

type Frames = Vec<(u8, Vec<u8>)>;

async fn start_server(
    instance: cassie::app::Cassie,
    config: CassieRuntimeConfig,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<Result<(), CassieError>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let address = listener.local_addr().expect("listener address");
    let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
        listener,
        std::sync::Arc::new(instance),
        config,
    ));
    (address, server)
}

async fn observe_target_activity<R, W>(reader: &mut R, writer: &mut W, target_pid: i32) -> Frames
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let sql =
        format!("SELECT state, query FROM pg_catalog.pg_stat_activity WHERE pid = {target_pid}");
    wire::write_frames(writer, vec![wire::simple_query_frame(&sql)]).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

fn assert_activity_marker(frames: &Frames, expected_state: &str, marker: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "pg_stat_activity control ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert!(
        wire::data_rows(frames).iter().any(|row| {
            row[0].as_deref() == Some(expected_state)
                && row[1]
                    .as_deref()
                    .is_some_and(|query| query.contains(marker))
        }),
        "target activity must be {expected_state} with marker {marker:?}: {:?}",
        wire::data_rows(frames)
    );
}

async fn wait_for_active_query_marker<R, W>(
    reader: &mut R,
    writer: &mut W,
    target_pid: i32,
    marker: &str,
) -> Frames
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let frames = observe_target_activity(reader, writer, target_pid).await;
        assert_eq!(
            wire::error_code(&frames),
            None,
            "pg_stat_activity query failed while waiting for target marker {marker:?}: {:?}",
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
        let rows = wire::data_rows(&frames);
        if rows.iter().any(|row| {
            row[0].as_deref() == Some("active")
                && row[1]
                    .as_deref()
                    .is_some_and(|query| query.contains(marker))
        }) {
            return frames;
        }
        assert!(
            Instant::now() < deadline,
            "target query with marker {marker:?} must be active before cancellation: {rows:?}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

struct JsonPathCancellationEvidence {
    baseline: Frames,
    baseline_activity: Frames,
    active_query: Frames,
    cancelled: Frames,
    recovery: Frames,
    cancel_elapsed: Duration,
}

async fn run_jsonpath_cancellation(
    instance: cassie::app::Cassie,
    config: CassieRuntimeConfig,
    query: &str,
    marker: &str,
) -> JsonPathCancellationEvidence {
    let (address, server) = start_server(instance, config).await;
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("JSONPath query connection");
    let (mut reader, mut writer) = tokio::io::split(socket);
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    let monitor_socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("JSONPath activity monitor connection");
    let (mut monitor_reader, mut monitor_writer) = tokio::io::split(monitor_socket);
    let _ = wire::complete_startup_with_backend_key(&mut monitor_reader, &mut monitor_writer).await;
    let control_marker = "issue_809_jsonpath_expansion_control";
    let control_query = format!(
        "/* {control_marker} */ SELECT COUNT(*) FROM \
         jsonb_path_query('[1,2,3]'::JSONB, '$[*]') AS left_item(value) \
         CROSS JOIN jsonb_path_query('[1,2,3]'::JSONB, '$[*]') AS right_item(value)"
    );

    wire::write_frames(&mut writer, vec![wire::simple_query_frame(&control_query)]).await;
    let baseline = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    assert_eq!(
        wire::error_code(&baseline),
        None,
        "valid JSONPath positive control must succeed before cancellation: {:?}",
        baseline
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let baseline_activity =
        observe_target_activity(&mut monitor_reader, &mut monitor_writer, process_id).await;
    assert_activity_marker(&baseline_activity, "idle", control_marker);

    wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
    let active_query =
        wait_for_active_query_marker(&mut monitor_reader, &mut monitor_writer, process_id, marker)
            .await;
    assert_activity_marker(&active_query, "active", marker);
    let mut cancel_socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect CancelRequest socket");
    let cancel_started = Instant::now();
    cancel_socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("send JSONPath cancellation request");
    cancel_socket
        .shutdown()
        .await
        .expect("close CancelRequest socket");
    let cancelled = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    let cancel_elapsed = cancel_started.elapsed();

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame("SELECT 45 AS recovery")],
    )
    .await;
    let recovery = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    drop(reader);
    drop(writer);
    drop(monitor_reader);
    drop(monitor_writer);
    server.abort();
    let _ = server.await;
    JsonPathCancellationEvidence {
        baseline,
        baseline_activity,
        active_query,
        cancelled,
        recovery,
        cancel_elapsed,
    }
}
