//! Shared pgwire fixtures for retained collection aggregate contracts.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

type Frames = Vec<(u8, Vec<u8>)>;

pub struct ResourceCase {
    pub control: Frames,
    pub wide_control: Frames,
    pub candidate: Frames,
    pub recovery: Frames,
}

pub struct CancellationCase {
    pub control: Frames,
    pub cancelled: Frames,
    pub recovery: Frames,
}

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct TestServer {
    address: SocketAddr,
    handle: tokio::task::JoinHandle<Result<(), CassieError>>,
}

impl TestServer {
    async fn stop(self) {
        self.handle.abort();
        let _ = self.handle.await;
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

fn configured_instance(path: &str, config: CassieRuntimeConfig) -> Cassie {
    let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("Cassie instance");
    cassie.startup().expect("start aggregate fixture");
    cassie
}

async fn start_server(instance: Arc<Cassie>, config: CassieRuntimeConfig) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let address = listener.local_addr().expect("pgwire listener address");
    drop(listener);
    let handle = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        instance,
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    TestServer { address, handle }
}

async fn connect(address: SocketAddr) -> (OwnedReadHalf, OwnedWriteHalf, i32, i32) {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("aggregate pgwire connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    (reader, writer, process_id, secret_key)
}

pub fn wide_input_query(aggregate_expression: &str) -> String {
    let values = (1..=10)
        .map(|value| format!("({value})"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "SELECT {aggregate_expression} \
         FROM (VALUES {values}) AS a(n) \
         CROSS JOIN (VALUES {values}) AS b(n) \
         CROSS JOIN (VALUES {values}) AS c(n) \
         CROSS JOIN (VALUES {values}) AS d(n) \
         CROSS JOIN (VALUES {values}) AS e(n)"
    )
}

fn marked_query(marker: &str, query: &str) -> String {
    format!("/* {marker} */ {query}")
}

async fn observe_target_activity<R, W>(reader: &mut R, writer: &mut W, target_pid: i32) -> Frames
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let sql = format!("SELECT state, query FROM pg_stat_activity WHERE pid = {target_pid}");
    wire::write_frames(writer, vec![wire::simple_query_frame(&sql)]).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

fn assert_activity_marker(frames: &Frames, expected_state: &str, marker: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "pg_stat_activity ErrorResponse: {:?}",
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
) where
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
            return;
        }
        assert!(
            Instant::now() < deadline,
            "target aggregate query with marker {marker:?} must be active before cancellation: {rows:?}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

pub fn run_memory_budget_case(
    label: &str,
    control_sql: &str,
    wide_control_sql: &str,
    candidate_sql: &str,
) -> ResourceCase {
    wire::use_local_storage();
    let path = wire::data_dir(label);
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 4_096;

    let result = runtime.block_on(async {
        let cassie = Arc::new(configured_instance(&path, config.clone()));
        let server = start_server(Arc::clone(&cassie), config).await;
        let (mut reader, mut writer, _, _) = connect(server.address).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame(control_sql)]).await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(wide_control_sql)],
        )
        .await;
        let wide_control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
        assert_no_error(&control, "small retained-value control");
        assert_no_error(&wide_control, "same-input nonretaining control");
        assert_control_result(&wide_control, "100000");
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(candidate_sql)]).await;
        let candidate =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT 46 AS recovery")],
        )
        .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;

        drop(reader);
        drop(writer);
        server.stop().await;
        ResourceCase {
            control,
            wide_control,
            candidate,
            recovery,
        }
    });
    drop(runtime);
    result
}

pub fn run_cancellation_case(
    label: &str,
    control_sql: &str,
    candidate_sql: &str,
) -> CancellationCase {
    wire::use_local_storage();
    let path = wire::data_dir(label);
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 512 * 1024 * 1024;

    let result = runtime.block_on(async {
        let cassie = Arc::new(configured_instance(&path, config.clone()));
        let server = start_server(Arc::clone(&cassie), config).await;
        let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;
        let (mut monitor_reader, mut monitor_writer, _, _) = connect(server.address).await;
        let control_marker = format!("{label}_control");
        let target_marker = format!("{label}_target");

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&marked_query(
                &control_marker,
                control_sql,
            ))],
        )
        .await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
        assert_no_error(&control, "small retained-value cancellation control");
        let control_activity =
            observe_target_activity(&mut monitor_reader, &mut monitor_writer, process_id).await;
        assert_activity_marker(&control_activity, "idle", &control_marker);

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&marked_query(
                &target_marker,
                candidate_sql,
            ))],
        )
        .await;
        wait_for_active_query_marker(
            &mut monitor_reader,
            &mut monitor_writer,
            process_id,
            &target_marker,
        )
        .await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.address)
            .await
            .expect("open cancellation connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("send aggregate cancellation request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancellation connection");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT 47 AS recovery")],
        )
        .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;

        drop(reader);
        drop(writer);
        drop(monitor_reader);
        drop(monitor_writer);
        server.stop().await;
        CancellationCase {
            control,
            cancelled,
            recovery,
        }
    });
    drop(runtime);
    result
}

pub fn assert_control_and_recovery(
    control: &Frames,
    expected_control: &str,
    recovery: &Frames,
    expected_recovery: &str,
) {
    assert_control_result(control, expected_control);
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(
        wire::data_rows(recovery),
        vec![vec![Some(expected_recovery.to_owned())]]
    );
}

pub fn assert_control_result(frames: &Frames, expected: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "valid PostgreSQL control failed: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(expected.to_owned())]]
    );
}

fn assert_no_error(frames: &Frames, description: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "{description} ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
}

pub fn assert_rejected_without_partial_rows(frames: &Frames, sqlstate: &str) {
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some(sqlstate),
        "aggregate ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
    assert_eq!(frames.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
}
