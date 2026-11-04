//! Ordered-set aggregate retained-state budget and cancellation over pgwire (#806).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

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

fn configured_instance(config: CassieRuntimeConfig) -> Arc<Cassie> {
    let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
    cassie
        .startup()
        .expect("start ordered-set aggregate fixture");
    Arc::new(cassie)
}

async fn start_server(instance: Arc<Cassie>, config: CassieRuntimeConfig) -> AggregateTestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ordered-set listener");
    let address = listener.local_addr().expect("listener address");
    let handle = tokio::spawn(cassie::pgwire::server::run_with_listener(
        listener, instance, config,
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

fn seed_ordered_set_input(cassie: &Cassie, table: &str, row_count: i32) {
    let session = cassie.create_session("tester", None);
    let create_table = format!("CREATE TABLE {table} (id INT4 PRIMARY KEY, value FLOAT8)");
    cassie
        .execute_sql(&session, &create_table, vec![])
        .unwrap_or_else(|error| panic!("create ordered-set input table {table}: {error}"));
    for first in (1..=row_count).step_by(250) {
        let last = (first + 249).min(row_count);
        let values = (first..=last)
            .map(|value| format!("({value}, {value}.0)"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("INSERT INTO {table} VALUES {values}");
        cassie
            .execute_sql(&session, &sql, vec![])
            .unwrap_or_else(|error| {
                panic!("insert ordered-set input table {table} batch {first}..={last}: {error}")
            });
    }
    drop(session);
}

fn seed_ordered_set_text_input(cassie: &Cassie, table: &str, value: &str) {
    let session = cassie.create_session("tester", None);
    let create_table = format!("CREATE TABLE {table} (value TEXT)");
    cassie
        .execute_sql(&session, &create_table, vec![])
        .unwrap_or_else(|error| panic!("create ordered-set text table {table}: {error}"));
    let insert = format!("INSERT INTO {table} VALUES ('{value}')");
    cassie
        .execute_sql(&session, &insert, vec![])
        .unwrap_or_else(|error| panic!("insert ordered-set text row {table}: {error}"));
    drop(session);
}

#[test]
fn should_reject_retained_state_growth_given_an_ordered_set_aggregate_when_its_memory_budget_is_exceeded(
) {
    // Arrange
    let runtime = current_thread_runtime();
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 128 * 1_024;

    // Act
    let (control, overflow, recovery) = runtime.block_on(async {
        let cassie = configured_instance(config.clone());
        seed_ordered_set_input(&cassie, "ordered_set_control_rows", 3);
        seed_ordered_set_input(&cassie, "ordered_set_budget_rows", 4_096);
        let server = start_server(Arc::clone(&cassie), config).await;
        let (mut reader, mut writer, _, _) = connect(server.address).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY value::FLOAT8) \
                 FROM ordered_set_control_rows",
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
fn should_reject_percentile_disc_array_output_when_its_memory_budget_is_exceeded() {
    // Arrange
    let runtime = current_thread_runtime();
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 40 * 1_024;

    // Act
    let (overflow, recovery) = runtime.block_on(async {
        let cassie = configured_instance(config.clone());
        let text = "x".repeat(12 * 1_024);
        seed_ordered_set_text_input(&cassie, "ordered_set_percentile_text", &text);
        let server = start_server(Arc::clone(&cassie), config).await;
        let (mut reader, mut writer, _, _) = connect(server.address).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT percentile_disc(ARRAY[0.25,0.75]) \
                 WITHIN GROUP (ORDER BY value) FROM ordered_set_percentile_text",
            )],
        )
        .await;
        let overflow =
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
        server.stop().await;
        (overflow, recovery)
    });
    drop(runtime);

    // Assert
    assert_eq!(
        wire::error_code(&overflow).as_deref(),
        Some("54000"),
        "percentile array output budget ErrorResponse: {:?}",
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
        .expect("percentile array memory-budget ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message.starts_with("query memory budget exceeded: ")
    }));
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("47".to_owned())]]
    );
}

#[test]
fn should_cancel_without_partial_rows_given_an_ordered_set_aggregate_when_cancel_request_arrives() {
    // Arrange
    use crate::support_collection_aggregate_resources as support;
    let query = "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY n::FLOAT8) FROM ordered_set_cancel_rows";
    let filtered = "SELECT percentile_cont(0.5) WITHIN GROUP (ORDER BY n::FLOAT8) FILTER (WHERE FALSE) FROM ordered_set_cancel_rows";
    for (rows, expected) in [(1, "1"), (3, "2")] {
        let setup = support::integer_table_fixture_setup("ordered_set_cancel_rows", rows);

        // Act
        let case = support::run_cancellation_case_with_setup_after_aggregate_input(
            "issue-806-percentile-retained-input",
            &setup,
            query,
            query,
            filtered,
        );

        // Assert
        support::assert_control_and_recovery(&case.control, expected, &case.recovery, "47");
        support::assert_cancelled_without_retained_resources(&case);
    }
}
