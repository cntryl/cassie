use std::sync::Arc;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

#[path = "support/pgwire.rs"]
mod pgwire;

const TEST_PASSWORD: &str = "cassie-network-test-password";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn password_config(password: &str) -> CassieRuntimeConfig {
    CassieRuntimeConfig {
        password: password.to_string(),
        ..CassieRuntimeConfig::default()
    }
}

async fn listener_startup_error(
    address: &str,
    cassie: Cassie,
    config: CassieRuntimeConfig,
) -> cassie::app::CassieError {
    tokio::time::timeout(
        Duration::from_secs(2),
        cassie::pgwire::server::run(address.to_string(), Arc::new(cassie), config),
    )
    .await
    .expect("listener validation should complete")
    .expect_err("unsafe listener must fail startup")
}

async fn failed_pgwire_auth_payload(address: std::net::SocketAddr) -> Vec<u8> {
    let socket = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect pgwire");
    let (mut reader, mut writer) = tokio::io::split(socket);
    tokio::io::AsyncWriteExt::write_all(
        &mut writer,
        &pgwire::startup_frame("postgres", "postgres"),
    )
    .await
    .expect("write startup");
    tokio::io::AsyncWriteExt::flush(&mut writer)
        .await
        .expect("flush startup");
    let authentication = pgwire::read_wire_frame(&mut reader).await;
    assert_eq!(authentication.0, b'R');
    tokio::io::AsyncWriteExt::write_all(&mut writer, &pgwire::password_message("wrong"))
        .await
        .expect("write password");
    tokio::io::AsyncWriteExt::flush(&mut writer)
        .await
        .expect("flush password");
    let error = pgwire::read_wire_frame(&mut reader).await;
    assert_eq!(error.0, b'E');
    error.1
}

#[test]
fn should_allow_passwordless_bootstrap_for_embedded_use_without_listeners() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("embedded-passwordless");
    let config = password_config("");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    cassie.startup().expect("startup");

    // Act
    let session = cassie.authenticate_role("postgres", None, None);

    // Assert
    assert!(session.is_ok());
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_passwordless_pgwire_listener_at_actual_loopback_address() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("listener-passwordless");
    let config = password_config("");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        // Act
        let error = listener_startup_error("localhost:0", cassie, config).await;

        // Assert
        assert!(error.to_string().contains("bootstrap password is empty"));
        assert!(error.to_string().contains("network listener"));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_passwordless_rest_listener_at_actual_loopback_address() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("rest-listener-passwordless");
    let config = password_config("");
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        // Act
        let error = tokio::time::timeout(
            Duration::from_secs(2),
            cassie::rest::router::run("127.0.0.1:0".to_string(), cassie),
        )
        .await
        .expect("listener validation should complete")
        .expect_err("passwordless REST listener must fail startup");

        // Assert
        assert!(error.to_string().contains("bootstrap password is empty"));
        assert!(error.to_string().contains("network listener"));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rotate_persisted_passwordless_role_before_pgwire_listener_startup() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("persisted-passwordless");
    {
        let cassie = Cassie::new_with_data_dir_and_config(&path, password_config(""))
            .expect("passwordless cassie");
        cassie.startup().expect("passwordless startup");
        cassie.shutdown();
    }
    let config = password_config(TEST_PASSWORD);
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        // Act
        let authenticated = cassie.authenticate_role("postgres", Some(TEST_PASSWORD), None);

        // Assert
        assert!(authenticated.is_ok());
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let shutdown_signal = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            shutdown_signal.notify_waiters();
        });
        cassie::pgwire::server::run_with_shutdown(
            "127.0.0.1:0".to_string(),
            Arc::new(cassie),
            config,
            shutdown,
        )
        .await
        .expect("rotated credentials permit pgwire listener");
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rotate_persisted_passwordless_role_before_rest_listener_startup() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("rest-persisted-passwordless");
    {
        let cassie = Cassie::new_with_data_dir_and_config(&path, password_config(""))
            .expect("passwordless cassie");
        cassie.startup().expect("passwordless startup");
        cassie.shutdown();
    }
    let config = password_config(TEST_PASSWORD);
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        // Act
        let authenticated = cassie.authenticate_role("postgres", Some(TEST_PASSWORD), None);

        // Assert
        assert!(authenticated.is_ok());
        let shutdown = Arc::new(tokio::sync::Notify::new());
        let shutdown_signal = shutdown.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            shutdown_signal.notify_waiters();
        });
        cassie::rest::router::run_with_shutdown("127.0.0.1:0".to_string(), cassie, shutdown)
            .await
            .expect("rotated credentials permit REST listener");
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_default_postgres_password_loopback_only_for_runtime_address() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("default-password-non-loopback");
    let config = CassieRuntimeConfig::default();
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        // Act
        let error = listener_startup_error("0.0.0.0:0", cassie, config).await;

        // Assert
        assert!(error
            .to_string()
            .contains("default bootstrap password is unsafe"));
        assert!(error.to_string().contains("0.0.0.0:0"));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_require_tls_for_actual_non_loopback_pgwire_listener() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("non-loopback-tls");
    let config = password_config(TEST_PASSWORD);
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        // Act
        let error = listener_startup_error("0.0.0.0:0", cassie, config).await;

        // Assert
        assert!(error.to_string().contains("pgwire TLS is required"));
        assert!(error.to_string().contains("0.0.0.0:0"));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_authenticate_non_empty_password_on_loopback_pgwire_listener() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("loopback-authenticated");
    let config = password_config(TEST_PASSWORD);
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
            address.to_string(),
            Arc::new(cassie),
            config,
            Arc::new(tokio::sync::Notify::new()),
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);

        // Act
        pgwire::complete_startup_with_password(&mut reader, &mut writer, TEST_PASSWORD).await;

        // Assert
        assert!(!server.is_finished());
        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_pgwire_authentication_envelope_generic_when_throttled() {
    // Arrange
    pgwire::with_fallback();
    let path = pgwire::data_dir("pgwire-auth-throttle");
    let config = CassieRuntimeConfig {
        password: TEST_PASSWORD.to_string(),
        auth_user_attempts_per_minute: 1,
        auth_ip_attempts_per_minute: 10,
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("cassie");
    cassie.startup().expect("startup");

    runtime().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run_with_shutdown(
            address.to_string(),
            Arc::new(cassie),
            config,
            Arc::new(tokio::sync::Notify::new()),
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Act
        let invalid = failed_pgwire_auth_payload(address).await;
        let throttled = failed_pgwire_auth_payload(address).await;

        // Assert
        assert_eq!(throttled, invalid);
        assert!(String::from_utf8_lossy(&throttled).contains("authentication failed"));
        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}
