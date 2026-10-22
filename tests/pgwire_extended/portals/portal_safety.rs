use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use tokio::io::BufReader;

use super::support_pgwire as support;

fn configured_cassie(label: &str, max_result_rows: usize) -> (Cassie, CassieRuntimeConfig, String) {
    configured_cassie_with_memory(
        label,
        max_result_rows,
        CassieRuntimeConfig::from_env()
            .expect("runtime config")
            .limits
            .query_memory_budget_bytes,
    )
}

fn configured_cassie_with_memory(
    label: &str,
    max_result_rows: usize,
    query_memory_budget_bytes: usize,
) -> (Cassie, CassieRuntimeConfig, String) {
    let path = support::data_dir(label);
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.max_result_rows = max_result_rows;
    config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("cassie");
    cassie.startup().expect("startup");
    (cassie, config, path)
}

fn seed_rows(cassie: &Cassie, table: &str, count: usize) {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (payload TEXT)"),
            vec![],
        )
        .expect("create table");
    for index in 0..count {
        cassie
            .midge
            .put_document(
                table,
                Some(format!("doc-{index:04}")),
                serde_json::json!({"payload": format!("value-{index:04}")}),
            )
            .expect("seed row");
    }
}

async fn spawn_server(
    cassie: Cassie,
    config: CassieRuntimeConfig,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let server = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        Arc::new(cassie),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    (address, server)
}

fn data_values(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .filter_map(|(_, payload)| {
            support::parse_data_row(payload)
                .into_iter()
                .next()
                .flatten()
        })
        .collect()
}

fn error_code(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    let (_, payload) = frames.iter().find(|(tag, _)| *tag == b'E')?;
    support::parse_error_fields(payload)
        .into_iter()
        .find_map(|(tag, value)| (tag == 'C').then_some(value))
}

#[test]
fn should_execute_i32_max_portal_page_without_capacity_sized_allocation() {
    // Arrange
    let (cassie, config, path) = configured_cassie("portal-i32-max", 16);
    seed_rows(&cassie, "portal_i32_max", 2);
    let observed = cassie.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (address, server) = spawn_server(cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        let before = observed.midge.query_scan_entries_for_diagnostics();

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("max_stmt", "SELECT payload FROM portal_i32_max"),
                support::bind_frame("max_portal", "max_stmt", &[]),
                support::execute_limited_frame("max_portal", i32::MAX),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready(&mut reader).await;
        let visited = observed
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(data_values(&frames).len(), 2);
        assert!(!frames.iter().any(|(tag, _)| *tag == b's'));
        assert_eq!(visited, 2);

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_stream_case_distinct_columns_without_crossing_identifier_keys() {
    // Arrange
    let (cassie, config, path) = configured_cassie("portal-case-distinct-columns", 16);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE portal_case_distinct (\"a\" INT, \"A\" INT)",
            vec![],
        )
        .expect("create case-distinct columns");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO portal_case_distinct (\"a\", \"A\") VALUES (11, 22), (33, 44)",
            vec![],
        )
        .expect("seed case-distinct rows");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (address, server) = spawn_server(cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        // Cross-Sync portals belong to an explicit transaction.
        support::transaction_control(&mut reader, &mut write_half, "BEGIN").await;

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame(
                    "case_distinct_stmt",
                    "SELECT \"a\", \"A\" FROM portal_case_distinct",
                ),
                support::bind_frame("case_distinct_portal", "case_distinct_stmt", &[]),
                support::execute_limited_frame("case_distinct_portal", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let first_page = support::read_frames_until_ready(&mut reader).await;
        support::write_frames(
            &mut write_half,
            vec![
                support::execute_limited_frame("case_distinct_portal", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let second_page = support::read_frames_until_ready(&mut reader).await;

        // Assert
        let mut rows = first_page
            .iter()
            .chain(&second_page)
            .filter(|(tag, _)| *tag == b'D')
            .map(|(_, payload)| support::parse_data_row(payload))
            .collect::<Vec<_>>();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                vec![Some("11".to_string()), Some("22".to_string())],
                vec![Some("33".to_string()), Some("44".to_string())],
            ]
        );
        assert_eq!(support::row_description_names(&first_page), vec!["a", "A"]);
        assert!(first_page.iter().any(|(tag, _)| *tag == b's'));
        assert!(second_page.iter().any(|(tag, _)| *tag == b'C'));

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enforce_result_row_cap_cumulatively_across_portal_resumes() {
    // Arrange
    let (cassie, config, path) = configured_cassie("portal-cumulative-cap", 3);
    seed_rows(&cassie, "portal_cumulative_cap", 5);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (address, server) = spawn_server(cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        // Cross-Sync portals belong to an explicit transaction.
        support::transaction_control(&mut reader, &mut write_half, "BEGIN").await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("cap_stmt", "SELECT payload FROM portal_cumulative_cap"),
                support::bind_frame("cap_portal", "cap_stmt", &[]),
                support::execute_limited_frame("cap_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        let first = support::read_frames_until_ready(&mut reader).await;
        assert_eq!(data_values(&first).len(), 2);
        assert!(first.iter().any(|(tag, _)| *tag == b's'));

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::execute_limited_frame("cap_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        let overflow = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(error_code(&overflow).as_deref(), Some("54000"));
        assert_eq!(
            data_values(&overflow).len(),
            0,
            "overflow page must be atomic"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_transaction_overlay_for_streaming_portal_results() {
    // Arrange
    let (cassie, config, path) = configured_cassie("portal-overlay", 16);
    seed_rows(&cassie, "portal_overlay", 1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (address, server) = spawn_server(cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        support::write_frames(&mut write_half, vec![support::simple_query_frame("BEGIN")]).await;
        let _ = support::read_frames_until_ready(&mut reader).await;
        support::write_frames(
            &mut write_half,
            vec![support::simple_query_frame(
                "INSERT INTO portal_overlay (payload) VALUES ('staged')",
            )],
        )
        .await;
        let inserted = support::read_frames_until_ready(&mut reader).await;
        assert!(inserted.iter().all(|(tag, _)| *tag != b'E'));

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("overlay_stmt", "SELECT payload FROM portal_overlay"),
                support::bind_frame("overlay_portal", "overlay_stmt", &[]),
                support::execute_limited_frame("overlay_portal", 8),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready(&mut reader).await;
        let values = data_values(&frames);

        // Assert
        assert_eq!(values.len(), 2);
        assert!(values.iter().any(|value| value == "staged"));

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}
