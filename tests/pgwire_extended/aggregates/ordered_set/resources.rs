//! Ordered-set aggregate retained-state budget and cancellation over pgwire (#806).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

struct AggregateFixtureDirectory(String);

impl Drop for AggregateFixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct AggregateTestServer {
    address: SocketAddr,
    handle: tokio::task::JoinHandle<Result<(), CassieError>>,
}

impl AggregateTestServer {
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
    cassie
        .startup()
        .expect("start ordered-set aggregate fixture");
    cassie
}

async fn start_server(instance: Cassie, config: CassieRuntimeConfig) -> AggregateTestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ordered-set listener");
    let address = listener.local_addr().expect("listener address");
    let handle = tokio::spawn(cassie::pgwire::server::run_with_listener(
        listener,
        Arc::new(instance),
        config,
    ));
    AggregateTestServer { address, handle }
}

async fn connect(address: SocketAddr) -> (OwnedReadHalf, OwnedWriteHalf, i32, i32) {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("ordered-set aggregate connection");
    let (mut reader, mut writer) = socket.into_split();
    let (process_id, secret_key) =
        wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;
    (reader, writer, process_id, secret_key)
}

fn seed_ordered_set_input(path: &str, row_count: i32) {
    let cassie = Cassie::new_with_data_dir(path).expect("create ordered-set input fixture");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE ordered_set_budget_rows (id INT4 PRIMARY KEY, value FLOAT8)",
            vec![],
        )
        .expect("create ordered-set input table");
    for first in (1..=row_count).step_by(250) {
        let last = (first + 249).min(row_count);
        let values = (first..=last)
            .map(|value| format!("({value}, {value}.0)"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("INSERT INTO ordered_set_budget_rows VALUES {values}");
        cassie
            .execute_sql(&session, &sql, vec![])
            .unwrap_or_else(|error| {
                panic!("insert ordered-set input batch {first}..={last}: {error}")
            });
    }
    drop(session);
    drop(cassie);
}

async fn wait_for_active_query_marker(
    reader: &mut OwnedReadHalf,
    writer: &mut OwnedWriteHalf,
    target_pid: i32,
    marker: &str,
) -> Vec<(u8, Vec<u8>)> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let sql = format!(
            "SELECT state, query FROM pg_catalog.pg_stat_activity WHERE pid = {target_pid}"
        );
        wire::write_frames(writer, vec![wire::simple_query_frame(&sql)]).await;
        let frames = wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await;
        assert_eq!(
            wire::error_code(&frames),
            None,
            "pg_stat_activity ErrorResponse while waiting for {marker:?}: {:?}",
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
            "ordered-set query with marker {marker:?} must be active before cancellation: {rows:?}"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

fn assert_active_query_marker(frames: &[(u8, Vec<u8>)], marker: &str) {
    assert!(
        wire::data_rows(frames).iter().any(|row| {
            row[0].as_deref() == Some("active")
                && row[1]
                    .as_deref()
                    .is_some_and(|query| query.contains(marker))
        }),
        "the target backend must be active with marker {marker:?}: {:?}",
        wire::data_rows(frames)
    );
}

#[test]
fn should_reject_retained_state_growth_given_an_ordered_set_aggregate_when_its_memory_budget_is_exceeded(
) {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("ordered-set-retained-state-budget");
    let _directory = AggregateFixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 4_096;
    // 1,024 FLOAT8 payloads alone exceed 4 KiB; throughput belongs in benches.
    seed_ordered_set_input(&path, 1_024);

    // Act
    let (control, overflow, recovery) = runtime.block_on(async {
        let cassie = configured_instance(&path, config.clone());
        let server = start_server(cassie, config).await;
        let (mut reader, mut writer, _, _) = connect(server.address).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY value::FLOAT8) \
                 FROM ordered_set_budget_rows WHERE id <= 3",
            )],
        )
        .await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY value::FLOAT8) \
                 FROM ordered_set_budget_rows",
            )],
        )
        .await;
        let overflow =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

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
        (control, overflow, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(
        wire::error_code(&control),
        None,
        "small ordered-set control ErrorResponse: {:?}",
        control
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::data_rows(&control), vec![vec![Some("2".to_owned())]]);
    assert_eq!(
        wire::error_code(&overflow).as_deref(),
        Some("54000"),
        "ordered-set state budget ErrorResponse: {:?}",
        overflow
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&overflow),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(overflow.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(overflow.last().map(|frame| frame.0), Some(b'Z'));
    let error = overflow
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("ordered-set memory-budget ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message.starts_with("query memory budget exceeded: ")
    }));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("46".to_owned())]]
    );
}

#[test]
fn should_cancel_without_partial_rows_given_an_ordered_set_aggregate_when_cancel_request_arrives() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("ordered-set-cancellation");
    let _directory = AggregateFixtureDirectory(path.clone());
    let runtime = current_thread_runtime();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_timeout_ms = 0;
    seed_ordered_set_input(&path, 3);

    // Act
    let (control, active_query, cancelled, recovery) = runtime.block_on(async {
        let cassie = configured_instance(&path, config.clone());
        let server = start_server(cassie, config).await;
        let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;
        let (mut observer_reader, mut observer_writer, _, _) = connect(server.address).await;
        let control_sql = "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY value) \
            FROM ordered_set_budget_rows WHERE id <= 3 AND pg_sleep(0) IS NOT NULL";
        let cancellable_sql = "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY value) \
            FROM ordered_set_budget_rows WHERE id <= 3 AND pg_sleep(60) IS NOT NULL \
            /* ordered_set_cancel */";

        wire::write_frames(&mut writer, vec![wire::simple_query_frame(control_sql)]).await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame(cancellable_sql)]).await;
        let active_query = wait_for_active_query_marker(
            &mut observer_reader,
            &mut observer_writer,
            process_id,
            "ordered_set_cancel",
        )
        .await;
        let mut cancel_socket = tokio::net::TcpStream::connect(server.address)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write ordered-set cancellation request");
        cancel_socket
            .shutdown()
            .await
            .expect("close ordered-set cancellation request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT 47 AS recovery")],
        )
        .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        drop(observer_reader);
        drop(observer_writer);
        server.stop().await;
        (control, active_query, cancelled, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(
        wire::error_code(&control),
        None,
        "small ordered-set cancellation control ErrorResponse: {:?}",
        control
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::data_rows(&control), vec![vec![Some("2".to_owned())]]);
    assert_active_query_marker(&active_query, "ordered_set_cancel");
    assert_eq!(
        wire::error_code(&cancelled).as_deref(),
        Some("57014"),
        "cancelled ordered-set ErrorResponse: {:?}",
        cancelled
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(&cancelled),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(cancelled.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("47".to_owned())]]
    );
}
