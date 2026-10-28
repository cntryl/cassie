//! Shared pgwire fixtures for retained collection aggregate contracts.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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

fn configured_instance(config: CassieRuntimeConfig) -> Cassie {
    Cassie::new_in_memory_with_config(config).expect("in-memory Cassie instance")
}

async fn start_server(instance: Arc<Cassie>, config: CassieRuntimeConfig) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let address = listener.local_addr().expect("pgwire listener address");
    let handle = tokio::spawn(cassie::pgwire::server::run_with_listener(
        listener, instance, config,
    ));
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

pub fn integer_table_fixture_setup(table_name: &str, row_count: i32) -> Vec<String> {
    let mut statements = vec![format!("CREATE TABLE {table_name} (n INT4)")];
    for first in (1..=row_count).step_by(250) {
        let last = (first + 249).min(row_count);
        let values = (first..=last)
            .map(|value| format!("({value})"))
            .collect::<Vec<_>>()
            .join(", ");
        statements.push(format!("INSERT INTO {table_name} VALUES {values}"));
    }
    statements
}

fn seed_fixture(cassie: &Cassie, setup_sql: &[String]) {
    if setup_sql.is_empty() {
        return;
    }

    let session = cassie.create_session("tester", None);
    for sql in setup_sql {
        cassie
            .execute_sql(&session, sql, vec![])
            .unwrap_or_else(|error| panic!("execute aggregate fixture setup {sql:?}: {error}"));
    }
    drop(session);
}

pub fn run_memory_budget_case_with_setup_at_budget(
    _label: &str,
    setup_sql: &[String],
    control_sql: &str,
    wide_control_sql: &str,
    candidate_sql: &str,
    budget_bytes: usize,
) -> ResourceCase {
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = budget_bytes;

    let result = runtime.block_on(async {
        let cassie = configured_instance(config.clone());
        seed_fixture(&cassie, setup_sql);
        cassie.startup().expect("start aggregate fixture");
        let cassie = Arc::new(cassie);
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

pub fn run_cancellation_case_with_setup_after_aggregate_input(
    _label: &str,
    setup_sql: &[String],
    control_sql: &str,
    candidate_sql: &str,
) -> CancellationCase {
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 512 * 1024 * 1024;

    runtime.block_on(async {
        let cassie = configured_instance(config.clone());
        seed_fixture(&cassie, setup_sql);
        cassie.startup().expect("start aggregate fixture");
        let cassie = Arc::new(cassie);
        let server = start_server(Arc::clone(&cassie), config).await;
        let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame(control_sql)]).await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;
        assert_no_error(&control, "small retained-value cancellation control");

        let (_pause_guard, selected, resume) =
            cassie::runtime::pause_next_query_after_aggregate_input();
        wire::write_frames(&mut writer, vec![wire::simple_query_frame(candidate_sql)]).await;
        tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
            .await
            .expect("aggregate-input pause observer task")
            .expect("target aggregate must retain input before cancellation");

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
            .expect("resume retained aggregate after CancelRequest was handled");
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
        server.stop().await;
        CancellationCase {
            control,
            cancelled,
            recovery,
        }
    })
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
