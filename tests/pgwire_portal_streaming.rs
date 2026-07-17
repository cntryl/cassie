use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use tokio::io::{AsyncWriteExt, BufReader};

#[path = "support/pgwire.rs"]
mod support;

#[test]
fn should_stop_portal_scan_after_requested_page_is_buffered() {
    // Arrange
    support::with_fallback();
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
    support::with_fallback();
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
    support::with_fallback();
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
