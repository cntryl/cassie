//! Minimal synchronized wire fixture for Boolean aggregate cancellation.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

type Frames = Vec<(u8, Vec<u8>)>;

pub struct CancellationCase {
    baseline: Frames,
    cancelled: Frames,
    recovery: Frames,
    dispatch: String,
    after_cancel: serde_json::Value,
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

pub fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

async fn start_server(instance: Arc<Cassie>, config: CassieRuntimeConfig) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let address = listener.local_addr().expect("listener address");
    let handle = tokio::spawn(cassie::pgwire::server::run_with_listener(
        listener, instance, config,
    ));
    TestServer { address, handle }
}

async fn connect(address: SocketAddr) -> (OwnedReadHalf, OwnedWriteHalf, i32, i32) {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("Boolean aggregate query connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    (reader, writer, process_id, secret_key)
}

async fn cancel_and_wait_for_close(address: SocketAddr, process_id: i32, secret_key: i32) {
    let mut socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("cancel connection");
    socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("write Boolean aggregate cancellation request");
    socket
        .shutdown()
        .await
        .expect("close Boolean aggregate cancellation request");
    let mut response = [0; 1];
    let received = tokio::time::timeout(Duration::from_secs(5), socket.read(&mut response))
        .await
        .expect("CancelRequest connection must close")
        .expect("read cancellation connection EOF");
    assert_eq!(received, 0, "CancelRequest has no response body");
}

async fn assert_empty_input_does_not_pause(
    reader: &mut OwnedReadHalf,
    writer: &mut OwnedWriteHalf,
    source: &str,
) {
    let queries = [
        format!("SELECT BOOL_AND(flag), BOOL_OR(flag) FROM {source} WHERE FALSE"),
        format!(
            "SELECT BOOL_AND(flag) FILTER (WHERE FALSE), BOOL_OR(flag) FILTER (WHERE FALSE) FROM {source}"
        ),
        format!(
            "SELECT BOOL_AND(flag) FILTER (WHERE NULL), BOOL_OR(flag) FILTER (WHERE NULL) FROM {source}"
        ),
    ];
    for sql in queries {
        let (_guard, selected, _resume) = cassie::runtime::pause_next_query_after_aggregate_input();
        wire::write_frames(writer, vec![wire::simple_query_frame(&sql)]).await;
        let frames = wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await;
        assert_eq!(wire::error_code(&frames), None);
        assert_eq!(wire::data_rows(&frames), vec![vec![None, None]]);
        assert!(
            matches!(selected.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
            "admission without selected aggregate input must not reach the retained-state boundary: {sql}"
        );
    }
}

pub async fn run_case(setup: &[&str], source: &str) -> CancellationCase {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_timeout_ms = 30_000;
    config.limits.parallel_scan_workers = 1;
    config.limits.parallel_aggregation_workers = 1;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).expect("Cassie"));
    cassie.startup().expect("start Boolean aggregate fixture");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("seed Boolean aggregate fixture");
    }
    drop(session);
    let server = start_server(Arc::clone(&cassie), config).await;
    let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;
    let sql =
        format!("SELECT BOOL_AND(flag) AS every_match, BOOL_OR(flag) AS any_match FROM {source}");
    wire::write_frames(&mut writer, vec![wire::simple_query_frame(&sql)]).await;
    let baseline = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    let dispatch = cassie.metrics()["parallel_aggregation"]["last_fallback_reason"]
        .as_str()
        .expect("aggregate dispatch reason")
        .to_owned();
    assert_empty_input_does_not_pause(&mut reader, &mut writer, source).await;

    let (_pause_guard, selected, resume) =
        cassie::runtime::pause_next_query_after_aggregate_input();
    wire::write_frames(&mut writer, vec![wire::simple_query_frame(&sql)]).await;
    tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
        .await
        .expect("Boolean input observer task")
        .expect("Boolean aggregate must retain input before cancellation");
    cancel_and_wait_for_close(server.address, process_id, secret_key).await;
    resume
        .send(())
        .expect("resume Boolean aggregate after cancellation was handled");
    let cancelled = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    let after_cancel = cassie.metrics();
    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame("SELECT 46 AS recovery")],
    )
    .await;
    let recovery = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    drop(reader);
    drop(writer);
    server.stop().await;
    CancellationCase {
        baseline,
        cancelled,
        recovery,
        dispatch,
        after_cancel,
    }
}

pub fn assert_case(case: &CancellationCase, expected_dispatch: &str) {
    assert_eq!(wire::error_code(&case.baseline), None);
    let description = case
        .baseline
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("baseline Boolean aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(&case.baseline),
        vec![vec![Some("f".to_owned()), Some("t".to_owned())]]
    );
    assert_eq!(case.dispatch, expected_dispatch);
    assert_eq!(wire::error_code(&case.cancelled).as_deref(), Some("57014"));
    assert_eq!(
        wire::data_rows(&case.cancelled),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        case.cancelled
            .iter()
            .filter(|frame| frame.0 == b'C')
            .count(),
        0
    );
    assert_eq!(case.cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(case.after_cancel["running_queries"].as_u64(), Some(0));
    assert_eq!(
        case.after_cancel["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        case.after_cancel["runtime"]["active_operator_workers"].as_u64(),
        Some(0)
    );
    assert_eq!(wire::error_code(&case.recovery), None);
    assert_eq!(
        wire::data_rows(&case.recovery),
        vec![vec![Some("46".to_owned())]]
    );
}
