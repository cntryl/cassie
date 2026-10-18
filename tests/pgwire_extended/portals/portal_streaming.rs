use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use tokio::io::{AsyncWriteExt, BufReader};

use super::support_pgwire as support;

#[test]
fn should_stop_portal_scan_after_requested_page_is_buffered() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-streaming-page");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_streaming_page (id TEXT, payload TEXT)",
                vec![],
            )
            .expect("create table");
        for index in 0..50 {
            cassie
                .midge
                .put_document(
                    "portal_streaming_page",
                    Some(format!("doc-{index:02}")),
                    serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
                )
                .expect("seed row");
        }
        let observed = cassie.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        let before = observed.midge.query_scan_entries_for_diagnostics();

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame(
                    "streaming_stmt",
                    "SELECT payload FROM portal_streaming_page",
                ),
                support::bind_frame("streaming_portal", "streaming_stmt", &[]),
                support::execute_limited_frame("streaming_portal", 2),
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
        assert!(frames.iter().any(|(tag, _)| *tag == b's'));
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'D').count(), 2);
        assert_eq!(visited, 3, "portal page should buffer one lookahead row");

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cancel_suspended_portal_before_resume() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-suspended-cancel");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_suspended_cancel (id TEXT, payload TEXT)",
                vec![],
            )
            .expect("create table");
        for index in 0..5 {
            cassie
                .midge
                .put_document(
                    "portal_suspended_cancel",
                    Some(format!("doc-{index:02}")),
                    serde_json::json!({"id": format!("doc-{index:02}"), "payload": "value"}),
                )
                .expect("seed row");
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        let (process_id, secret_key) =
            support::complete_startup_with_backend_key(&mut reader, &mut write_half).await;
        // Cross-Sync portals belong to an explicit transaction.
        support::transaction_control(&mut reader, &mut write_half, "BEGIN").await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("cancel_stmt", "SELECT payload FROM portal_suspended_cancel"),
                support::bind_frame("cancel_portal", "cancel_stmt", &[]),
                support::execute_limited_frame("cancel_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        let initial = support::read_frames_until_ready(&mut reader).await;
        assert!(initial.iter().any(|(tag, _)| *tag == b's'));

        // Act
        let mut cancel_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&support::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        tokio::time::sleep(Duration::from_millis(50)).await;
        support::write_frames(
            &mut write_half,
            vec![
                support::execute_limited_frame("cancel_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        let resumed = support::read_frames_until_ready(&mut reader).await;

        // Assert
        let error = resumed
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("suspended portal cancellation error");
        let fields = support::parse_error_fields(&error.1);
        assert!(fields
            .iter()
            .any(|(tag, value)| *tag == 'C' && value == "57014"));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_resume_portal_from_original_snapshot_after_concurrent_insert() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-snapshot-resume");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_snapshot_rows (value TEXT)",
                vec![],
            )
            .expect("create table");
        for id in ["doc-a", "doc-b", "doc-c"] {
            cassie
                .midge
                .put_document(
                    "portal_snapshot_rows",
                    Some(id.to_string()),
                    serde_json::json!({"value": id}),
                )
                .expect("seed row");
        }
        let observed = cassie.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        // Cross-Sync portals belong to an explicit transaction.
        support::transaction_control(&mut reader, &mut write_half, "BEGIN").await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("snapshot_stmt", "SELECT id FROM portal_snapshot_rows"),
                support::bind_frame("snapshot_portal", "snapshot_stmt", &[]),
                support::execute_limited_frame("snapshot_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        let initial = support::read_frames_until_ready(&mut reader).await;
        assert_eq!(
            data_values(&initial),
            vec!["doc-a".to_string(), "doc-b".to_string()]
        );
        observed
            .midge
            .put_document(
                "portal_snapshot_rows",
                Some("doc-aa".to_string()),
                serde_json::json!({"value": "doc-aa"}),
            )
            .expect("concurrent insert");

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::execute_limited_frame("snapshot_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        let resumed = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(data_values(&resumed), vec!["doc-c".to_string()]);
        assert!(!resumed.iter().any(|(tag, _)| *tag == b's'));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_align_extended_wildcard_data_rows_with_row_description_when_table_declares_id() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("extended-wildcard-declared-id");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (cassie, row_id) = declared_id_table(&path, "extended_wildcard_id_docs");
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("wildcard_stmt", "SELECT * FROM extended_wildcard_id_docs"),
                support::describe_statement_frame("wildcard_stmt"),
                support::bind_frame("wildcard_portal", "wildcard_stmt", &[]),
                support::execute_frame("wildcard_portal"),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
        assert_eq!(
            support::row_description_names(&frames),
            vec!["id".to_string(), "title".to_string()]
        );
        assert_eq!(
            support::data_rows(&frames),
            vec![vec![Some(row_id), Some("x".to_string())]]
        );

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_align_streaming_portal_wildcard_data_rows_with_row_description_when_table_declares_id() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-wildcard-declared-id");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (cassie, row_id) = declared_id_table(&path, "portal_wildcard_id_docs");
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("wildcard_stmt", "SELECT * FROM portal_wildcard_id_docs"),
                support::bind_frame("wildcard_portal", "wildcard_stmt", &[]),
                support::describe_portal_frame("wildcard_portal"),
                support::execute_limited_frame("wildcard_portal", 10),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
        assert_eq!(
            support::row_description_names(&frames),
            vec!["id".to_string(), "title".to_string()]
        );
        assert_eq!(
            support::data_rows(&frames),
            vec![vec![Some(row_id), Some("x".to_string())]]
        );

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

/// Pipelines Parse, Bind, Describe(Statement), a row-limited Execute and
/// Sync for `sql` over a three-row table and returns the frame tags.
fn describe_statement_after_bind_tags(label: &str, sql: &str) -> Vec<u8> {
    support::use_local_storage();
    let path = support::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE dab_names (name TEXT)",
            "INSERT INTO dab_names (name) VALUES ('alpha'), ('beta'), ('gamma')",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("dab_stmt", sql),
                support::bind_frame("dab_portal", "dab_stmt", &[]),
                support::describe_statement_frame("dab_stmt"),
                support::execute_limited_frame("dab_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready(&mut reader).await;
        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(&path);
        frames.into_iter().map(|(tag, _)| tag).collect()
    })
}

#[test]
fn should_send_one_row_description_given_describe_statement_after_bind_on_streaming_portal() {
    // Arrange
    let sql = "SELECT name FROM dab_names";

    // Act
    let tags = describe_statement_after_bind_tags("describe-after-bind-streaming", sql);

    // Assert
    assert_eq!(tags, b"12tTDDsZ".to_vec());
}

#[test]
fn should_send_one_row_description_given_describe_statement_after_bind_on_materialized_portal() {
    // Arrange
    let sql = "SELECT name FROM dab_names ORDER BY name";

    // Act
    let tags = describe_statement_after_bind_tags("describe-after-bind-materialized", sql);

    // Assert
    assert_eq!(tags, b"12tTDDsZ".to_vec());
}

/// Seeds `count` rows of `{"payload": "value-NN"}` straight through the
/// storage adapter, bypassing SQL so the test only exercises paging.
fn seed_payload_rows(cassie: &Cassie, collection: &str, count: usize) {
    for index in 0..count {
        cassie
            .midge
            .put_document(
                collection,
                Some(format!("doc-{index:02}")),
                serde_json::json!({"payload": format!("value-{index:02}")}),
            )
            .expect("seed row");
    }
}

/// Runs simple queries on an established connection, asserting none error.
async fn run_simple_queries(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    write_half: &mut tokio::net::tcp::WriteHalf<'_>,
    statements: &[&str],
) {
    for sql in statements {
        support::write_frames(write_half, vec![support::simple_query_frame(sql)]).await;
        let frames = support::read_frames_until_ready(reader).await;
        assert!(frames.iter().all(|(tag, _)| *tag != b'E'), "{sql} failed");
    }
}

/// Commits `sql` from a separate connection so the suspended portal under
/// test sees a concurrent write land between two Execute messages.
async fn commit_delete_from_second_connection(addr: std::net::SocketAddr, sql: &str) {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("writer connection");
    let (read_half, mut write_half) = socket.split();
    let mut reader = BufReader::new(read_half);
    support::complete_startup(&mut reader, &mut write_half).await;
    support::write_frames(&mut write_half, vec![support::simple_query_frame(sql)]).await;
    let frames = support::read_frames_until_ready(&mut reader).await;
    assert!(frames.iter().all(|(tag, _)| *tag != b'E'), "delete failed");
}

#[test]
fn should_page_portal_from_one_result_when_delete_commits_during_staged_transaction() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-staged-concurrent-delete");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_staged_rows (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_payload_rows(&cassie, "portal_staged_rows", 6);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut reader_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("reader connection");
        let (read_half, mut write_half) = reader_socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        run_simple_queries(
            &mut reader,
            &mut write_half,
            &[
                "BEGIN",
                "INSERT INTO portal_staged_rows (payload) VALUES ('staged')",
            ],
        )
        .await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("staged_stmt", "SELECT payload FROM portal_staged_rows"),
                support::bind_frame("staged_portal", "staged_stmt", &[]),
                support::execute_limited_frame("staged_portal", 3),
                support::sync_frame(),
            ],
        )
        .await;
        let first_page = data_values(&support::read_frames_until_ready(&mut reader).await);
        commit_delete_from_second_connection(
            addr,
            "DELETE FROM portal_staged_rows WHERE payload = 'value-00'",
        )
        .await;

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::execute_limited_frame("staged_portal", 100),
                support::sync_frame(),
            ],
        )
        .await;
        let second_page = data_values(&support::read_frames_until_ready(&mut reader).await);

        // Assert
        assert_eq!(first_page.len(), 3, "first page = {first_page:?}");
        let mut all_pages = first_page
            .iter()
            .chain(second_page.iter())
            .cloned()
            .collect::<Vec<_>>();
        all_pages.sort();
        let mut expected = (0..6)
            .map(|index| format!("value-{index:02}"))
            .chain(std::iter::once("staged".to_string()))
            .collect::<Vec<_>>();
        expected.sort();
        assert_eq!(
            all_pages, expected,
            "portal pages must come from one result; first={first_page:?} second={second_page:?}"
        );

        drop(reader_socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_page_wildcard_portal_from_one_result_when_source_has_no_collection_schema() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-schemaless-concurrent-create");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for name in ["portal_catalog_b", "portal_catalog_c", "portal_catalog_d"] {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {name} (payload TEXT)"),
                    vec![],
                )
                .expect("create table");
        }
        let observed = cassie.clone();
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        // Cross-Sync portals belong to an explicit transaction.
        support::transaction_control(&mut reader, &mut write_half, "BEGIN").await;
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame("catalog_stmt", "SELECT * FROM information_schema.tables"),
                support::bind_frame("catalog_portal", "catalog_stmt", &[]),
                support::describe_portal_frame("catalog_portal"),
                support::execute_limited_frame("catalog_portal", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let first_frames = support::read_frames_until_ready(&mut reader).await;
        observed
            .execute_sql(
                &session,
                "CREATE TABLE portal_catalog_a (payload TEXT)",
                vec![],
            )
            .expect("concurrent create table");

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::execute_limited_frame("catalog_portal", 1000),
                support::sync_frame(),
            ],
        )
        .await;
        let second_frames = support::read_frames_until_ready(&mut reader).await;

        // Assert
        let names = support::row_description_names(&first_frames);
        let table_name = names
            .iter()
            .position(|name| name == "table_name")
            .unwrap_or_else(|| panic!("table_name column: {names:?} {first_frames:?}"));
        let first_page = support::data_rows(&first_frames);
        let second_page = support::data_rows(&second_frames);
        let table_names = first_page
            .iter()
            .chain(second_page.iter())
            .filter_map(|row| row[table_name].clone())
            .filter(|name| name.starts_with("portal_catalog_"))
            .collect::<Vec<_>>();
        assert_eq!(
            table_names,
            vec![
                "portal_catalog_b".to_string(),
                "portal_catalog_c".to_string(),
                "portal_catalog_d".to_string(),
            ],
            "first={first_page:?} second={second_page:?}"
        );

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_page_limited_portal_over_user_view() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-user-view-limited-page");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE portal_view_docs (title TEXT, score FLOAT)",
                "INSERT INTO portal_view_docs (title, score) VALUES ('alpha', 3.0), ('bravo', 4.0), ('charlie', 5.0), ('hidden', 1.0)",
                "CREATE VIEW portal_view_high AS SELECT title FROM portal_view_docs WHERE score > 2.0",
            ] {
                cassie.execute_sql(&session, sql, vec![]).expect("seed view");
            }
            let server = support::spawn_server(cassie).await;
            let mut socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("query connection");
            let (read_half, mut write_half) = socket.split();
            let mut reader = BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;

            // Act
            support::write_frames(
                &mut write_half,
                vec![
                    support::parse_frame("user_view_stmt", "SELECT title FROM portal_view_high"),
                    support::bind_frame("user_view_portal", "user_view_stmt", &[]),
                    support::execute_limited_frame("user_view_portal", 2),
                    support::sync_frame(),
                ],
            )
            .await;
            let frames = support::read_frames_until_ready(&mut reader).await;

            // Assert
            assert_eq!(
                frames.iter().filter(|(tag, _)| *tag == b'E').count(),
                0,
                "limited portal over a user view must not error: {frames:?}"
            );
            assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'D').count(), 2);
            assert!(frames.iter().any(|(tag, _)| *tag == b's'));

            drop(socket);
            server.stop().await;
            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_page_limited_portal_over_virtual_view_column() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-virtual-view-limited-page");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_catalog_limited (payload TEXT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE portal_catalog_limited_second (payload TEXT)",
                vec![],
            )
            .expect("create second table");
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("query connection");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;

        // Act
        support::write_frames(
            &mut write_half,
            vec![
                support::parse_frame(
                    "virtual_view_stmt",
                    "SELECT table_name FROM information_schema.tables",
                ),
                support::bind_frame("virtual_view_portal", "virtual_view_stmt", &[]),
                support::execute_limited_frame("virtual_view_portal", 1),
                support::sync_frame(),
            ],
        )
        .await;
        let frames = support::read_frames_until_ready(&mut reader).await;

        // Assert
        assert_eq!(
            frames.iter().filter(|(tag, _)| *tag == b'E').count(),
            0,
            "limited portal over a virtual view must not error: {frames:?}"
        );
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'D').count(), 1);
        assert!(frames.iter().any(|(tag, _)| *tag == b's'));

        drop(socket);
        server.stop().await;
        let _ = std::fs::remove_dir_all(path);
    });
}

fn declared_id_table(path: &str, table: &str) -> (Cassie, String) {
    let cassie = Cassie::new_with_data_dir(path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        format!("CREATE TABLE {table} (id TEXT, title TEXT)"),
        format!("INSERT INTO {table} (id, title) VALUES ('a', 'x')"),
    ] {
        cassie
            .execute_sql(&session, &sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
    let selected = cassie
        .execute_sql(&session, &format!("SELECT id FROM {table}"), vec![])
        .expect("select row id");
    let cassie::types::Value::String(row_id) = selected.rows()[0][0].clone() else {
        panic!("row id should be text");
    };
    (cassie, row_id)
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
