//! Aggregate resource bounds and cancellation outcomes through PostgreSQL wire protocol.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

type Frames = Vec<(u8, Vec<u8>)>;

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
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

async fn start_server(instance: Cassie, config: CassieRuntimeConfig) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind pgwire listener");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let server = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        Arc::new(instance),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    TestServer {
        address,
        handle: server,
    }
}

async fn connect(address: SocketAddr) -> (OwnedReadHalf, OwnedWriteHalf, i32, i32) {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("aggregate query connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    (reader, writer, process_id, secret_key)
}

fn configured_instance(path: &str, config: CassieRuntimeConfig) -> Cassie {
    let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("Cassie");
    cassie.startup().expect("start aggregate fixture");
    cassie
}

fn sum_over_value_product(cardinality: usize) -> String {
    let values = (1..=cardinality)
        .map(|value| format!("({value})"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "SELECT SUM(a.n + b.n + c.n) AS total \
         FROM (VALUES {values}) AS a(n) \
         CROSS JOIN (VALUES {values}) AS b(n) \
         CROSS JOIN (VALUES {values}) AS c(n)"
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
            "target aggregate query with marker {marker:?} must be active before cancellation: {rows:?}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[test]
fn should_bound_aggregate_rows_without_publishing_partial_output() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-804-aggregate-result-limit");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 2;

    // Act
    let (setup, bounded, recovery) = runtime.block_on(async {
        let cassie = configured_instance(&path, config.clone());
        let server = start_server(cassie, config).await;
        let (mut reader, mut writer, _, _) = connect(server.address).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "CREATE TABLE issue_804_bounded_aggregate_rows (bucket TEXT, value INT); \
                 INSERT INTO issue_804_bounded_aggregate_rows VALUES \
                 ('a',1),('b',2),('c',3)",
            )],
        )
        .await;
        let setup = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT bucket, COUNT(*) FROM issue_804_bounded_aggregate_rows \
                 GROUP BY bucket ORDER BY bucket",
            )],
        )
        .await;
        let bounded =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT 45 AS recovery")],
        )
        .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.stop().await;
        (setup, bounded, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(wire::error_code(&setup), None);
    assert_eq!(wire::error_code(&bounded).as_deref(), Some("54000"));
    assert_eq!(wire::data_rows(&bounded), Vec::<Vec<Option<String>>>::new());
    assert_eq!(bounded.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(bounded.last().map(|frame| frame.0), Some(b'Z'));
    let bounded_error = bounded
        .iter()
        .find(|frame| frame.0 == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("bounded aggregate ErrorResponse");
    assert!(bounded_error.iter().any(|(kind, message)| {
        *kind == 'M' && message == "query result row limit exceeded: 3 > 2"
    }));

    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("45".to_owned())]]
    );
}

struct AggregateCancellationEvidence {
    baseline: Frames,
    baseline_activity: Frames,
    active_query: Frames,
    cancelled: Frames,
    recovery: Frames,
    cancel_elapsed: Duration,
}

fn run_aggregate_cancellation(
    path: &str,
    config: CassieRuntimeConfig,
) -> AggregateCancellationEvidence {
    let runtime = current_thread_runtime();
    let evidence = runtime.block_on(async {
        let cassie = configured_instance(path, config.clone());
        let server = start_server(cassie, config).await;
        let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;
        let (mut monitor_reader, mut monitor_writer, _, _) = connect(server.address).await;
        let control_marker = "issue_804_aggregate_cancellation_control";
        let target_marker = "issue_804_aggregate_cancellation_target";

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&marked_query(
                control_marker,
                &sum_over_value_product(5),
            ))],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        assert_eq!(
            wire::error_code(&baseline),
            None,
            "valid aggregate positive control must succeed before cancellation: {:?}",
            baseline
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
        let baseline_activity =
            observe_target_activity(&mut monitor_reader, &mut monitor_writer, process_id).await;
        assert_activity_marker(&baseline_activity, "idle", control_marker);

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&marked_query(
                target_marker,
                &sum_over_value_product(256),
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
            .expect("cancel connection");
        let cancel_started = Instant::now();
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write aggregate cancellation request");
        cancel_socket
            .shutdown()
            .await
            .expect("close aggregate cancellation request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        let cancel_elapsed = cancel_started.elapsed();

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT 46 AS recovery")],
        )
        .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        drop(monitor_reader);
        drop(monitor_writer);
        server.stop().await;
        AggregateCancellationEvidence {
            baseline,
            baseline_activity,
            active_query,
            cancelled,
            recovery,
            cancel_elapsed,
        }
    });
    drop(runtime);
    evidence
}

#[test]
fn should_cancel_aggregate_input_without_publishing_partial_output() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-804-aggregate-cancellation");
    let _directory = FixtureDirectory(path.clone());
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 2;
    config.limits.query_timeout_ms = 0;

    // Act
    let evidence = run_aggregate_cancellation(&path, config);

    // Assert
    assert_eq!(wire::error_code(&evidence.baseline), None);
    assert_activity_marker(
        &evidence.baseline_activity,
        "idle",
        "issue_804_aggregate_cancellation_control",
    );
    assert_activity_marker(
        &evidence.active_query,
        "active",
        "issue_804_aggregate_cancellation_target",
    );
    let baseline_description = evidence
        .baseline
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("baseline SUM RowDescription");
    assert_eq!(
        wire::parse_row_description(&baseline_description.1)[0].type_oid,
        20
    );
    // PostgreSQL 18.6 returns 1125 for SUM(a.n + b.n + c.n) over 1..5 cubed.
    assert_eq!(
        wire::data_rows(&evidence.baseline),
        vec![vec![Some("1125".to_owned())]]
    );

    assert_eq!(
        wire::error_code(&evidence.cancelled).as_deref(),
        Some("57014")
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
    assert!(
        evidence.cancel_elapsed < Duration::from_secs(2),
        "explicit cancellation should stop aggregate input before the query deadline: {:?}",
        evidence.cancel_elapsed
    );

    assert_eq!(wire::error_code(&evidence.recovery), None);
    assert_eq!(
        wire::data_rows(&evidence.recovery),
        vec![vec![Some("46".to_owned())]]
    );
}
