//! Aggregate resource bounds and cancellation outcomes through PostgreSQL wire protocol.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

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
    assert!(wire::data_rows(&bounded).is_empty());
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

#[test]
fn should_cancel_aggregate_input_without_publishing_partial_output() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-804-aggregate-cancellation");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 2;
    config.limits.query_timeout_ms = 3_000;

    // Act
    let (baseline, cancelled, recovery, cancel_elapsed) = runtime.block_on(async {
        let cassie = configured_instance(&path, config.clone());
        let server = start_server(cassie, config).await;
        let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&sum_over_value_product(5))],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(&sum_over_value_product(256))],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
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
        (baseline, cancelled, recovery, cancel_elapsed)
    });
    drop(runtime);

    // Assert
    assert_eq!(wire::error_code(&baseline), None);
    let baseline_description = baseline
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("baseline SUM RowDescription");
    assert_eq!(
        wire::parse_row_description(&baseline_description.1)[0].type_oid,
        20
    );
    // PostgreSQL 18.6 returns 1125 for SUM(a.n + b.n + c.n) over 1..5 cubed.
    assert_eq!(
        wire::data_rows(&baseline),
        vec![vec![Some("1125".to_owned())]]
    );

    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert!(wire::data_rows(&cancelled).is_empty());
    assert_eq!(cancelled.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert!(
        cancel_elapsed < Duration::from_secs(2),
        "explicit cancellation should stop aggregate input before the query deadline: {cancel_elapsed:?}"
    );

    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("46".to_owned())]]
    );
}
