#[path = "support/pgwire.rs"]
mod pgwire;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use pgwire::{complete_startup, parse_error_fields, read_wire_frame};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
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
    with_fallback();
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
    with_fallback();
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
