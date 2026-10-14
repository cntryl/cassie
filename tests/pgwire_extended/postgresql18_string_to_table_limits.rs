//! PostgreSQL string_to_table result caps through pgwire, tracked by #803.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::{SqlFixture, sql_fixture_with_config};
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;

fn configured_fixture(label: &str, maximum_rows: usize) -> (SqlFixture, CassieRuntimeConfig) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = maximum_rows;
    let storage = sql_fixture_with_config(label, &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start string_to_table result-cap fixture");
    (storage, config)
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

async fn start_server(
    instance: Cassie,
    config: CassieRuntimeConfig,
) -> (SocketAddr, tokio::task::JoinHandle<Result<(), CassieError>>) {
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
    (address, server)
}

#[test]
fn should_cap_string_to_table_pgwire_results_without_partial_rows() {
    // Arrange
    let (storage, config) = configured_fixture("issue-803-string-to-table-result-cap", 2);
    let runtime = current_thread_runtime();

    // Act
    let (within_limit, overflow, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT value FROM string_to_table('a,b', ',') AS expanded(value)",
            )],
        )
        .await;
        let within_limit =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT value FROM string_to_table('a,b,c', ',') AS expanded(value)",
            )],
        )
        .await;
        let overflow =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 42")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (within_limit, overflow, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(wire::error_code(&within_limit), None);
    let description = within_limit
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("string_to_table RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(&within_limit),
        vec![vec![Some("a".to_owned())], vec![Some("b".to_owned())]]
    );

    // PostgreSQL 18.6 returns three text rows for this input. Cassie's configured
    // result-row cap rejects the result atomically when the third row is reached.
    assert_eq!(wire::error_code(&overflow).as_deref(), Some("54000"));
    assert!(wire::data_rows(&overflow).is_empty());
    assert_eq!(overflow.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(overflow.last().map(|frame| frame.0), Some(b'Z'));
    let error = overflow
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("string_to_table result-cap ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|field| field.0 == 'M')
            .map(|(_, message)| message.as_str()),
        Some("query result row limit exceeded: 3 > 2")
    );

    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("42".to_owned())]]
    );
}
