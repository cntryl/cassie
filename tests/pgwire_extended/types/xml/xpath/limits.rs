//! Bounded and cancellable `XPath` result expansion over pgwire.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

struct XPathCancellationEvidence {
    control: Frames,
    control_activity: Frames,
    active_query: Frames,
    cancelled: Frames,
    recovery: Frames,
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
        .expect("runtime")
}

fn configured_instance(path: &str, config: CassieRuntimeConfig) -> Cassie {
    let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("Cassie");
    cassie.startup().expect("start XPath fixture");
    cassie
}

async fn start_server(instance: Cassie, config: CassieRuntimeConfig) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind XPath fixture listener");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let handle = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        Arc::new(instance),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    TestServer { address, handle }
}

async fn connect(
    address: SocketAddr,
    database: &str,
    password: &str,
) -> (
    tokio::net::tcp::OwnedReadHalf,
    tokio::net::tcp::OwnedWriteHalf,
    i32,
    i32,
) {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("XPath query connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_as(&mut reader, &mut writer, "root", database, password).await;
    (reader, writer, process_id, secret_key)
}

fn xpath_sql(item_count: usize, aggregate: bool) -> String {
    let document = format!("<root>{}</root>", "<i/>".repeat(item_count));
    if aggregate {
        format!(
            "SELECT COUNT(*) FROM unnest(xpath('/root/i', XMLPARSE(DOCUMENT '{document}'))) AS expanded(value)"
        )
    } else {
        format!("SELECT xpath('/root/i', XMLPARSE(DOCUMENT '{document}'))")
    }
}

fn error_fields(frames: &[(u8, Vec<u8>)]) -> Vec<(char, String)> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .unwrap_or_default()
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
        error_fields(frames)
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
            error_fields(&frames)
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
            "target XPath query with marker {marker:?} must be active before cancellation: {rows:?}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn run_xpath_cancellation(
    path: &str,
    config: CassieRuntimeConfig,
    database: &str,
    password: &str,
) -> XPathCancellationEvidence {
    let cassie = configured_instance(path, config.clone());
    let server = start_server(cassie, config).await;
    let (mut reader, mut writer, process_id, secret_key) =
        connect(server.address, database, password).await;
    let (mut monitor_reader, mut monitor_writer, _, _) =
        connect(server.address, database, password).await;
    let control_marker = "issue_906_xpath_cancellation_control";
    let target_marker = "issue_906_xpath_cancellation_target";

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(&marked_query(
            control_marker,
            &xpath_sql(3, true),
        ))],
    )
    .await;
    let control = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    assert_eq!(
        wire::error_code(&control),
        None,
        "valid XPath positive control must succeed before cancellation: {:?}",
        error_fields(&control)
    );
    let control_activity =
        observe_target_activity(&mut monitor_reader, &mut monitor_writer, process_id).await;
    assert_activity_marker(&control_activity, "idle", control_marker);

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame(&marked_query(
            target_marker,
            &xpath_sql(100_000, true),
        ))],
    )
    .await;
    let active_query = wait_for_active_query_marker(
        &mut monitor_reader,
        &mut monitor_writer,
        process_id,
        target_marker,
    )
    .await;
    assert_activity_marker(&active_query, "active", target_marker);
    let mut cancel_socket = tokio::net::TcpStream::connect(server.address)
        .await
        .expect("XPath cancel connection");
    cancel_socket
        .write_all(&wire::cancel_request_frame(process_id, secret_key))
        .await
        .expect("write XPath cancel request");
    cancel_socket
        .shutdown()
        .await
        .expect("close XPath cancel request");
    let cancelled =
        wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(10)).await;

    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame("SELECT 49 AS recovery")],
    )
    .await;
    let recovery = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

    drop(reader);
    drop(writer);
    drop(monitor_reader);
    drop(monitor_writer);
    server.stop().await;
    XPathCancellationEvidence {
        control,
        control_activity,
        active_query,
        cancelled,
        recovery,
    }
}

#[test]
fn should_reject_xpath_expansion_given_a_memory_budget_when_result_growth_exceeds_the_budget() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("xpath-expansion-memory-budget");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 65_536;
    let database = config.database.clone();
    let password = config.password.clone();

    // Act
    let (control, bounded, recovery) = runtime.block_on(async {
        let cassie = configured_instance(&path, config.clone());
        let server = start_server(cassie, config).await;
        let (mut reader, mut writer, _, _) = connect(server.address, &database, &password).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&xpath_sql(3, false))],
        )
        .await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&xpath_sql(10_000, false))],
        )
        .await;
        let bounded =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT 48 AS recovery")],
        )
        .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.stop().await;
        (control, bounded, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(
        wire::error_code(&control),
        None,
        "small XPath control ErrorResponse: {:?}",
        error_fields(&control)
    );
    let control_description = control
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XPath control RowDescription");
    assert_eq!(
        wire::parse_row_description(&control_description.1)[0].type_oid,
        143
    );
    assert_eq!(
        wire::data_rows(&control),
        vec![vec![Some("{<i/>,<i/>,<i/>}".to_owned())]]
    );
    assert_eq!(
        wire::error_code(&bounded).as_deref(),
        Some("54000"),
        "XPath memory-budget ErrorResponse: {:?}",
        error_fields(&bounded)
    );
    assert_eq!(wire::data_rows(&bounded), Vec::<Vec<Option<String>>>::new());
    assert_eq!(bounded.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(bounded.last().map(|(tag, _)| *tag), Some(b'Z'));
    assert!(error_fields(&bounded).iter().any(|(kind, message)| {
        *kind == 'M' && message.starts_with("query memory budget exceeded: ")
    }));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("48".to_owned())]]
    );
}

#[test]
fn should_cancel_xpath_without_partial_output_given_expanding_results_when_cancel_request_arrives()
{
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("xpath-expansion-cancellation");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 64 * 1024 * 1024;
    config.limits.max_result_rows = 200_000;
    let database = config.database.clone();
    let password = config.password.clone();

    // Act
    let evidence = runtime.block_on(run_xpath_cancellation(&path, config, &database, &password));
    drop(runtime);

    // Assert
    assert_eq!(
        wire::error_code(&evidence.control),
        None,
        "small XPath cancellation control ErrorResponse: {:?}",
        error_fields(&evidence.control)
    );
    assert_activity_marker(
        &evidence.control_activity,
        "idle",
        "issue_906_xpath_cancellation_control",
    );
    assert_activity_marker(
        &evidence.active_query,
        "active",
        "issue_906_xpath_cancellation_target",
    );
    assert_eq!(
        wire::data_rows(&evidence.control),
        vec![vec![Some("3".to_owned())]]
    );
    assert_eq!(
        wire::error_code(&evidence.cancelled).as_deref(),
        Some("57014"),
        "cancelled XPath ErrorResponse: {:?}",
        error_fields(&evidence.cancelled)
    );
    assert_eq!(
        wire::data_rows(&evidence.cancelled),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        evidence
            .cancelled
            .iter()
            .filter(|(tag, _)| *tag == b'C')
            .count(),
        0
    );
    assert_eq!(evidence.cancelled.last().map(|(tag, _)| *tag), Some(b'Z'));
    assert_eq!(wire::error_code(&evidence.recovery), None);
    assert_eq!(
        wire::data_rows(&evidence.recovery),
        vec![vec![Some("49".to_owned())]]
    );
}
