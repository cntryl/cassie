//! Bounded, cancellable JSON row expansion and atomic failure publication.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use tokio::io::AsyncWriteExt;

use crate::support_pgwire as wire;
use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use crate::support_type_metadata_contract as fixture;

fn configured_fixture(label: &str, maximum_rows: usize) -> (SqlFixture, CassieRuntimeConfig) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = maximum_rows;
    let storage = sql_fixture_with_config(label, &[], config.clone());
    storage
        .cassie
        .startup()
        .expect("start JSON expansion fixture");
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
fn should_cap_json_expansion_results_without_partial_rows() {
    // Arrange
    let (storage, config) = configured_fixture("json-expansion-result-cap", 2);
    let runtime = current_thread_runtime();

    // Act
    let (batches, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let queries = [
            "SELECT value FROM jsonb_array_elements('[1,2,3]'::JSONB) AS expanded(value)",
            "SELECT value FROM jsonb_path_query('[1,2,3]'::JSONB, '$[*]') AS expanded(value)",
            "SELECT jsonb_object_keys('{\"a\":1,\"b\":2,\"c\":3}'::JSONB)",
        ];
        let mut batches = Vec::with_capacity(queries.len());
        for query in queries {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
            batches.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }
        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 42")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (batches, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(batches.len(), 3);
    for frames in &batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("54000"));
        assert!(wire::data_rows(frames).is_empty());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
    }
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_resume_json_array_expansion_across_bounded_portal_pages() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types(
            "json_array_page",
            "SELECT value FROM jsonb_array_elements('[1,2,3,4]'::JSONB) AS expanded(value)",
            &[],
        ),
        wire::describe_statement_frame("json_array_page"),
        wire::bind_frame("json_array_page_portal", "json_array_page", &[]),
        wire::describe_portal_frame("json_array_page_portal"),
        wire::execute_limited_frame("json_array_page_portal", 2),
        wire::execute_limited_frame("json_array_page_portal", 2),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("json-array-bounded-portal-pages", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3_802, -1, -1), 0, true);
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b's').count(), 1);
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 1);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("2".to_owned())],
            vec![Some("3".to_owned())],
            vec![Some("4".to_owned())],
        ]
    );
}

#[test]
fn should_cancel_json_array_expansion_without_publishing_a_partial_row() {
    // Arrange
    let (storage, config) = configured_fixture("json-array-expansion-cancel", 100);
    let runtime = current_thread_runtime();

    // Act
    let (baseline, cancelled, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("query connection");
        let (mut reader, mut writer) = tokio::io::split(socket);
        let (process_id, secret_key) =
            wire::complete_startup_with_backend_key(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT value FROM jsonb_array_elements('[1]'::JSONB) AS expanded(value)",
            )],
        )
        .await;
        let baseline =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT value FROM jsonb_array_elements('[1,2,3]'::JSONB) AS expanded(value) WHERE pg_sleep_for(INTERVAL '60 seconds') IS NOT NULL",
            )],
        )
        .await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        let mut cancel_socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&wire::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let cancelled =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 45")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (baseline, cancelled, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(wire::error_code(&baseline), None);
    assert_eq!(wire::data_rows(&baseline), vec![vec![Some("1".to_owned())]]);
    assert_eq!(wire::error_code(&cancelled).as_deref(), Some("57014"));
    assert!(wire::data_rows(&cancelled).is_empty());
    assert_eq!(cancelled.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("45".to_owned())]]
    );
}
