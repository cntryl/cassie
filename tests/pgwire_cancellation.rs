use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncWriteExt, BufReader};

#[path = "support/pgwire.rs"]
mod support;

#[test]
fn should_cancel_active_pgwire_query_with_matching_backend_key() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("cancel-active-query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        config.limits.cte_recursion_depth = 1_000_000;
        config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
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
        let mut query_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("query connection");
        let (query_read, mut query_write) = query_socket.split();
        let mut query_reader = BufReader::new(query_read);
        let (process_id, secret_key) =
            support::complete_startup_with_backend_key(&mut query_reader, &mut query_write).await;
        query_write
            .write_all(&support::simple_query_frame(
                "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000) SELECT MAX(n) FROM seq",
            ))
            .await
            .expect("write long query");
        query_write.flush().await.expect("flush long query");
        tokio::time::sleep(Duration::from_millis(25)).await;

        // Act
        let mut cancel_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&support::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write cancel request");
        cancel_socket.shutdown().await.expect("close cancel request");
        let frames = tokio::time::timeout(
            Duration::from_secs(5),
            support::read_frames_until_ready(&mut query_reader),
        )
        .await
        .expect("cancelled query should finish promptly");

        // Assert
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("query cancellation error");
        let fields = support::parse_error_fields(&error.1);
        assert!(fields
            .iter()
            .any(|(tag, value)| *tag == 'C' && value == "57014"));

        drop(query_socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_pgwire_cancel_request_while_backend_is_idle() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("cancel-idle-query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
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
        let mut query_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("query connection");
        let (query_read, mut query_write) = query_socket.split();
        let mut query_reader = BufReader::new(query_read);
        let (process_id, secret_key) =
            support::complete_startup_with_backend_key(&mut query_reader, &mut query_write).await;

        // Act
        let mut cancel_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("cancel connection");
        cancel_socket
            .write_all(&support::cancel_request_frame(process_id, secret_key))
            .await
            .expect("write idle cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        query_write
            .write_all(&support::simple_query_frame("SELECT 1"))
            .await
            .expect("write query after idle cancellation");
        query_write.flush().await.expect("flush query");
        let frames = support::read_frames_until_ready(&mut query_reader).await;

        // Assert
        assert!(frames.iter().all(|(tag, _)| *tag != b'E'));
        assert!(frames.iter().any(|(tag, _)| *tag == b'D'));

        drop(query_socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
