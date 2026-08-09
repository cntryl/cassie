#[path = "support/pgwire.rs"]
mod pgwire;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use pgwire::{complete_startup, parse_error_fields, read_wire_frame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

fn use_local_storage() {
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-admission-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

fn runtime_config_with_limits(pgwire_max: usize, rest_max: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password = "postgres".to_string();
    config.limits.pgwire_max_connections = pgwire_max;
    config.limits.rest_max_connections = rest_max;
    config
}

fn tls_identity(label: &str) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("cassie-admission-tls-{label}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).expect("create TLS directory");
    let certificate = directory.join("cert.pem");
    let key = directory.join("key.pem");
    let identity = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("certificate identity");
    std::fs::write(&certificate, identity.cert.pem()).expect("certificate fixture");
    std::fs::write(&key, identity.signing_key.serialize_pem()).expect("key fixture");
    (directory, certificate, key)
}

fn error_field(payload: &[u8], field: char) -> Option<String> {
    parse_error_fields(payload)
        .into_iter()
        .find_map(|(tag, value)| (tag == field).then_some(value))
}

async fn read_http_response_head(stream: &mut tokio::net::TcpStream) -> String {
    let mut response = Vec::new();
    let mut buf = [0u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut buf).await.expect("read http byte");
        response.push(buf[0]);
    }
    String::from_utf8(response).expect("http response should be utf-8")
}

#[test]
fn should_reject_pgwire_connections_over_admission_limit() {
    // Arrange
    use_local_storage();
    let path = data_dir("pgwire");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = runtime_config_with_limits(1, 512);
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut held = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect held pgwire");
        {
            let (mut held_reader, mut held_writer) = held.split();
            complete_startup(&mut held_reader, &mut held_writer).await;
        }

        // Act
        let mut overflow = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect overflow pgwire");
        let (tag, payload) = read_wire_frame(&mut overflow).await;

        // Assert
        assert_eq!(tag, b'E');
        assert_eq!(error_field(&payload, 'C').as_deref(), Some("53300"));
        assert_eq!(error_field(&payload, 'S').as_deref(), Some("FATAL"));

        drop(held);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut later = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect later pgwire");
        let (mut later_reader, mut later_writer) = later.split();
        complete_startup(&mut later_reader, &mut later_writer).await;

        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_release_rest_admission_permit_after_overflow_503() {
    // Arrange
    use_local_storage();
    let path = data_dir("rest");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = runtime_config_with_limits(256, 1);
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut held = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect held rest");
        held.write_all(
            b"GET /health HTTP/1.1\r\nhost: localhost\r\nconnection: keep-alive\r\n\r\n",
        )
        .await
        .expect("write held request");
        let held_head = read_http_response_head(&mut held).await;
        assert!(held_head.starts_with("HTTP/1.1 200"), "{held_head}");

        let client = reqwest::Client::new();

        // Act
        let overflow = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .expect("overflow request");
        let overflow_status = overflow.status();
        let overflow_connection = overflow
            .headers()
            .get(reqwest::header::CONNECTION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let overflow_body = overflow.text().await.expect("overflow body");

        // Assert
        assert_eq!(overflow_status.as_u16(), 503);
        assert_eq!(overflow_connection.as_deref(), Some("close"));
        assert!(
            overflow_body.contains("too many connections"),
            "body={overflow_body}"
        );

        drop(held);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let later = client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .expect("later request");
        assert!(later.status().is_success());

        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_emit_no_plaintext_pgwire_rejection_before_required_tls() {
    // Arrange
    use_local_storage();
    let path = data_dir("pgwire-tls-overflow");
    let (tls_dir, certificate, key) = tls_identity("pgwire-overflow");
    let mut config = runtime_config_with_limits(1, 512);
    config.password = "non-default-secret".to_string();
    config.pgwire_tls_cert_file = Some(certificate.to_string_lossy().to_string());
    config.pgwire_tls_key_file = Some(key.to_string_lossy().to_string());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("0.0.0.0:0")
            .await
            .expect("reserve listener");
        let port = listener.local_addr().expect("listener address").port();
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            format!("0.0.0.0:{port}"),
            std::sync::Arc::new(cassie),
            config,
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let held = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect held pgwire");

        // Act
        let mut overflow = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect overflow pgwire");
        let mut byte = [0_u8; 1];
        let observed = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            overflow.read(&mut byte),
        )
        .await;

        // Assert
        assert!(
            matches!(observed, Ok(Ok(0) | Err(_))),
            "TLS-required admission rejection exposed plaintext: {observed:?}"
        );
        drop(held);
        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
    let _ = std::fs::remove_dir_all(tls_dir);
}

#[test]
fn should_return_rest_admission_503_over_tls() {
    // Arrange
    use_local_storage();
    let path = data_dir("rest-tls-overflow");
    let (tls_dir, certificate, key) = tls_identity("rest-overflow");
    let mut config = runtime_config_with_limits(256, 1);
    config.rest_tls_cert_file = Some(certificate.to_string_lossy().to_string());
    config.rest_tls_key_file = Some(key.to_string_lossy().to_string());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let held_client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("held TLS client");
        let held = held_client
            .get(format!("https://{addr}/health"))
            .send()
            .await
            .expect("held TLS request");
        assert!(held.status().is_success());
        let overflow_client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("overflow TLS client");

        // Act
        let overflow = overflow_client
            .get(format!("https://{addr}/health"))
            .send()
            .await;

        // Assert
        let overflow = overflow.expect("TLS admission rejection response");
        assert_eq!(overflow.status().as_u16(), 503);
        assert!(overflow
            .text()
            .await
            .expect("overflow body")
            .contains("too many connections"));
        drop(held);
        server.abort();
        let _ = server.await;
    });

    let _ = std::fs::remove_dir_all(path);
    let _ = std::fs::remove_dir_all(tls_dir);
}
