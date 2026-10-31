//! Aggregate resource bounds and cancellation outcomes through PostgreSQL wire protocol.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

type Frames = Vec<(u8, Vec<u8>)>;

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
        .expect("aggregate query connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    (reader, writer, process_id, secret_key)
}

fn configured_instance(config: CassieRuntimeConfig) -> Arc<Cassie> {
    let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
    cassie.startup().expect("start aggregate fixture");
    Arc::new(cassie)
}

fn sum_with_sleep(interval: &str) -> String {
    format!(
        "SELECT SUM(CASE WHEN pg_sleep_for(INTERVAL '{interval}') IS NULL \
         THEN 0 ELSE value END) AS total \
         FROM (VALUES (1), (2)) AS inputs(value)"
    )
}

async fn wait_for_running_query(cassie: &Cassie, baseline: u64) -> u64 {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let running = cassie.metrics()["running_queries"]
            .as_u64()
            .expect("running query metric");
        if running > baseline {
            return running;
        }
        assert!(
            Instant::now() < deadline,
            "aggregate query must be admitted before cancellation"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[test]
fn should_bound_aggregate_rows_without_publishing_partial_output() {
    // Arrange
    let runtime = current_thread_runtime();
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.max_result_rows = 2;

    // Act
    let (created, inserted, bounded, recovery) = runtime.block_on(async {
        let cassie = configured_instance(config.clone());
        let server = start_server(cassie, config).await;
        let (mut reader, mut writer, _, _) = connect(server.address).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "CREATE TABLE issue_804_bounded_aggregate_rows (bucket TEXT, value INT)",
            )],
        )
        .await;
        let created =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "INSERT INTO issue_804_bounded_aggregate_rows VALUES \
                 ('a',1),('b',2),('c',3)",
            )],
        )
        .await;
        let inserted =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

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
        (created, inserted, bounded, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(wire::error_code(&created), None);
    assert_eq!(wire::error_code(&inserted), None);
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
    cancelled: Frames,
    recovery: Frames,
    cancel_elapsed: Duration,
}

fn run_aggregate_cancellation(config: CassieRuntimeConfig) -> AggregateCancellationEvidence {
    let runtime = current_thread_runtime();
    let evidence = runtime.block_on(async {
        let cassie = configured_instance(config.clone());
        let server = start_server(Arc::clone(&cassie), config).await;
        let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&sum_with_sleep("0 seconds"))],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        let baseline_running_queries = cassie.metrics()["running_queries"]
            .as_u64()
            .expect("running query metric");

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&sum_with_sleep("60 seconds"))],
        )
        .await;
        let active_queries = wait_for_running_query(&cassie, baseline_running_queries).await;
        assert_eq!(active_queries, baseline_running_queries + 1);

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
        server.stop().await;
        AggregateCancellationEvidence {
            baseline,
            cancelled,
            recovery,
            cancel_elapsed,
        }
    });
    drop(runtime);
    evidence
}

#[test]
fn should_cancel_aggregate_query_without_publishing_partial_output() {
    // Arrange
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.max_result_rows = 2;
    config.limits.query_timeout_ms = 30_000;

    // Act
    let evidence = run_aggregate_cancellation(config);

    // Assert
    assert_eq!(wire::error_code(&evidence.baseline), None);
    let baseline_description = evidence
        .baseline
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("baseline SUM RowDescription");
    assert_eq!(
        wire::parse_row_description(&baseline_description.1)[0].type_oid,
        20
    );
    assert_eq!(
        wire::data_rows(&evidence.baseline),
        vec![vec![Some("3".to_owned())]]
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
        "explicit cancellation should interrupt the aggregate query before its deadline: {:?}",
        evidence.cancel_elapsed
    );

    assert_eq!(wire::error_code(&evidence.recovery), None);
    assert_eq!(
        wire::data_rows(&evidence.recovery),
        vec![vec![Some("46".to_owned())]]
    );
}
