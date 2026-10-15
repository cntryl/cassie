//! Scalar-subquery result-cap rejection releases resources and preserves the session (#762).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::sql_fixture_with_config;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;

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

fn assert_result_limit_recovery(
    control: &[(u8, Vec<u8>)],
    scalar_control: &[(u8, Vec<u8>)],
    overflow: &[(u8, Vec<u8>)],
    recovery: &[(u8, Vec<u8>)],
) {
    assert_eq!(wire::error_code(control), None);
    assert_eq!(
        wire::data_rows(control),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
    assert_eq!(
        wire::error_code(scalar_control),
        None,
        "within-cap scalar-subquery ErrorResponse: {:?}",
        scalar_control
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(scalar_control),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ]
    );
    assert_eq!(
        wire::error_code(overflow).as_deref(),
        Some("54000"),
        "over-budget scalar-subquery ErrorResponse: {:?}",
        overflow
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::data_rows(overflow), Vec::<Vec<Option<String>>>::new());
    assert_eq!(overflow.iter().filter(|frame| frame.0 == b'C').count(), 0);
    assert_eq!(overflow.last().map(|frame| frame.0), Some(b'Z'));
    let error = overflow
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("scalar-subquery result-cap ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message == "query result row limit exceeded: 3 > 2"
    }));
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(
        wire::data_rows(recovery),
        vec![vec![Some("42".to_owned())]],
        "the session remains usable after rejecting the over-budget result"
    );
}

#[test]
fn should_release_resources_given_an_over_budget_subquery_when_execution_is_rejected() {
    // Arrange
    wire::use_local_storage();
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = 2;
    let fixture = sql_fixture_with_config(
        "scalar-subquery-result-limit-recovery",
        &[
            "CREATE TABLE scalar_limit_outer (id INT4 PRIMARY KEY)",
            "CREATE TABLE scalar_limit_inner (id INT4 PRIMARY KEY, value INT4)",
            "INSERT INTO scalar_limit_outer VALUES (1), (2), (3)",
            "INSERT INTO scalar_limit_inner VALUES (1, 10), (2, 20), (3, 30)",
        ],
        config.clone(),
    );
    fixture
        .cassie
        .startup()
        .expect("start result-limit fixture");
    let runtime = current_thread_runtime();

    // Act
    let (control, scalar_control, overflow, recovery) = runtime.block_on(async {
        let (address, server) = start_server(fixture.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT id FROM scalar_limit_outer WHERE id <= 2 ORDER BY id",
            )],
        )
        .await;
        let control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT outer_row.id, \
                        (SELECT inner_row.value FROM scalar_limit_inner AS inner_row \
                         WHERE inner_row.id = outer_row.id) AS scalar_value \
                 FROM scalar_limit_outer AS outer_row \
                 WHERE outer_row.id <= 2 ORDER BY outer_row.id",
            )],
        )
        .await;
        let scalar_control =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT outer_row.id, \
                        (SELECT inner_row.value FROM scalar_limit_inner AS inner_row \
                         WHERE inner_row.id = outer_row.id) AS scalar_value \
                 FROM scalar_limit_outer AS outer_row ORDER BY outer_row.id",
            )],
        )
        .await;
        let overflow =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame("SELECT 42 AS recovery")],
        )
        .await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (control, scalar_control, overflow, recovery)
    });
    drop(runtime);
    drop(fixture);

    // Assert
    assert_result_limit_recovery(&control, &scalar_control, &overflow, &recovery);
}
