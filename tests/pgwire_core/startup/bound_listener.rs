//! Listener ownership and transport validation without startup timing assumptions.

use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

use crate::support_pgwire as wire;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

fn authenticated_config() -> CassieRuntimeConfig {
    CassieRuntimeConfig {
        password: crate::support_sql::TEST_ROOT_PASSWORD.to_owned(),
        ..CassieRuntimeConfig::default()
    }
}

#[test]
fn should_serve_authenticated_queries_given_an_owned_listener_when_clients_connect_immediately() {
    // Arrange
    let path = wire::data_dir("bound-listener-startup");
    let config = authenticated_config();
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).expect("Cassie"));
    cassie.startup().expect("startup");
    runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        let shutdown = Arc::new(tokio::sync::Notify::new());

        // Act
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener_and_shutdown(
            listener,
            cassie,
            config,
            Arc::clone(&shutdown),
        ));
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect without a startup wait");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 7")]).await;
        let frames =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
        shutdown.notify_one();
        let stopped = tokio::time::timeout(Duration::from_secs(5), server)
            .await
            .expect("shutdown completes")
            .expect("server task");

        // Assert
        assert_eq!(wire::error_code(&frames), None);
        assert_eq!(wire::data_rows(&frames), vec![vec![Some("7".to_owned())]]);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        stopped.expect("graceful server shutdown");
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_insecure_transport_given_a_wildcard_listener_when_serving_is_requested() {
    // Arrange
    let path = wire::data_dir("bound-listener-transport");
    let config = CassieRuntimeConfig {
        password: "bound-listener-test-password".to_owned(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).expect("Cassie"));
    let error = runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
            .await
            .expect("bind wildcard listener");

        // Act
        cassie::pgwire::server::run_with_listener(listener, cassie, config)
            .await
            .expect_err("actual listener requires TLS")
    });

    // Assert
    assert!(error.to_string().contains("TLS"), "{error}");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_invalid_tls_identity_given_an_owned_listener_when_serving_is_requested() {
    // Arrange
    let path = wire::data_dir("bound-listener-invalid-tls");
    let mut config = authenticated_config();
    config.pgwire_tls_cert_file = Some(format!("{path}/missing-cert.pem"));
    config.pgwire_tls_key_file = Some(format!("{path}/missing-key.pem"));
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).expect("Cassie"));
    let error = runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");

        // Act
        cassie::pgwire::server::run_with_listener(listener, cassie, config)
            .await
            .expect_err("invalid identity cannot serve clients")
    });

    // Assert
    assert!(
        error.to_string().contains("pgwire TLS certificate"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(path);
}
