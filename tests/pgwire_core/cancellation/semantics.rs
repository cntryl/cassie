use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

use super::support_pgwire as support;

#[test]
fn should_cancel_only_the_backend_given_matching_process_id_and_secret() {
    // Arrange
    let path = support::data_dir("cancel-active-query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        crate::support_sql::test_root_password().clone_into(&mut config.password);
        let cassie = Cassie::new_in_memory_with_config(config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
            listener,
            Arc::new(cassie),
            config,
        ));
        let mut query_socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("query connection");
        let (query_read, mut query_write) = query_socket.split();
        let mut query_reader = BufReader::new(query_read);
        let (process_id, secret_key) =
            support::complete_startup_with_backend_key(&mut query_reader, &mut query_write).await;
        let (_pause_guard, selected, resume) = cassie::runtime::pause_next_query_at_sleep_entry();
        query_write
            .write_all(&support::simple_query_frame("SELECT pg_sleep(30)"))
            .await
            .expect("write sleeping query");
        query_write.flush().await.expect("flush sleeping query");
        tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
            .await
            .expect("sleep-entry observer task")
            .expect("query must reach the paused sleep function");

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
        let mut cancel_response = [0; 1];
        let cancel_response_bytes = cancel_socket
            .read(&mut cancel_response)
            .await
            .expect("wait for matching cancel request to be handled");
        assert_eq!(
            cancel_response_bytes, 0,
            "CancelRequest has no response body"
        );
        resume.send(()).expect("resume query after cancellation");
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
fn should_ignore_a_cancel_request_given_a_stale_backend_secret() {
    // Arrange
    let path = support::data_dir("cancel-idle-query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        crate::support_sql::test_root_password().clone_into(&mut config.password);
        let cassie = Cassie::new_in_memory_with_config(config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener(
            listener,
            Arc::new(cassie),
            config,
        ));
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
            .write_all(&support::cancel_request_frame(
                process_id,
                secret_key.wrapping_add(1),
            ))
            .await
            .expect("write idle cancel request");
        cancel_socket
            .shutdown()
            .await
            .expect("close cancel request");
        let mut cancel_response = Vec::new();
        cancel_socket
            .read_to_end(&mut cancel_response)
            .await
            .expect("wait for idle cancel request processing");
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
