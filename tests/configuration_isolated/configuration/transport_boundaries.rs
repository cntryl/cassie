use super::support_environment as environment;
use super::support_pgwire as pgwire_support;

use environment::EnvironmentGuard;

use std::net::SocketAddr;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use pgwire_support::{
    data_dir, password_message, read_until_ready, read_wire_frame, simple_query_frame,
    startup_frame, use_local_storage,
};

type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

async fn read_auth_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, Vec<u8>) {
    read_wire_frame(reader).await
}

fn read_boundary_counter(
    metrics: &serde_json::Value,
    interface: &str,
    kind: &str,
    op: &str,
) -> u64 {
    metrics[interface][kind][op].as_u64().unwrap_or_default()
}

fn seed_transport_boundary_docs(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "transport_boundary_docs");
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .unwrap();
    cassie.register_collection(&collection, schema);
    cassie
        .midge
        .put_document(
            &collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .unwrap();
}

async fn spawn_pgwire_boundary_server(cassie: &Cassie) -> (SocketAddr, PgwireServer) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password = "postgres".to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("listener address");
    drop(listener);

    let server = tokio::spawn(cassie::pgwire::server::run(
        addr.to_string(),
        std::sync::Arc::new(cassie.clone()),
        config,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    (addr, server)
}

async fn start_pgwire_session(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(writer, &startup_frame("root", "postgres"))
        .await
        .expect("startup write");
    let auth_frame = read_auth_frame(reader).await;
    if auth_request_code(&auth_frame.1) == Some(3) {
        let password =
            std::env::var("CASSIE_ROOT_PASSWORD").unwrap_or_else(|_| "postgres".to_string());
        tokio::io::AsyncWriteExt::write_all(writer, &password_message(&password))
            .await
            .expect("password write");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush password");
        let auth_ok = read_auth_frame(reader).await;
        assert_eq!(auth_request_code(&auth_ok.1), Some(0));
    }
    let _ready = read_until_ready(reader).await;
}

async fn run_pgwire_boundary_query(addr: SocketAddr) {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect pgwire");
    let (read_half, mut write_half) = socket.split();
    let mut reader = tokio::io::BufReader::new(read_half);
    start_pgwire_session(&mut reader, &mut write_half).await;

    tokio::io::AsyncWriteExt::write_all(
        &mut write_half,
        &simple_query_frame("SELECT title FROM transport_boundary_docs ORDER BY title"),
    )
    .await
    .expect("simple query write");
    tokio::io::AsyncWriteExt::flush(&mut write_half)
        .await
        .expect("flush query");

    loop {
        let frame = read_wire_frame(&mut reader).await;
        if frame.0 == b'Z' {
            break;
        }
    }
}

fn assert_pgwire_boundary_metrics(metrics: &serde_json::Value) {
    let started = read_boundary_counter(
        metrics,
        "pgwire",
        "blocking_started_total",
        "pgwire_simple_query",
    );
    let completed = read_boundary_counter(
        metrics,
        "pgwire",
        "blocking_completed_total",
        "pgwire_simple_query",
    );
    let errors = read_boundary_counter(
        metrics,
        "pgwire",
        "blocking_error_total",
        "pgwire_simple_query",
    );
    let join_failed = read_boundary_counter(
        metrics,
        "pgwire",
        "blocking_join_failed_total",
        "pgwire_simple_query",
    );
    assert_eq!(
        metrics["pgwire"]["simple_queries_total"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    assert_eq!(started, 1);
    assert_eq!(completed, 1);
    assert_eq!(errors, 0);
    assert_eq!(join_failed, 0);
    assert!(
        metrics["pgwire"]["blocking_elapsed_ms_total"]
            .get("pgwire_simple_query")
            .is_some(),
        "elapsed metric should be present"
    );
}

fn auth_request_code(payload: &[u8]) -> Option<i32> {
    payload
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map(i32::from_be_bytes)
}

#[test]
fn should_record_pgwire_blocking_boundary_metrics_for_simple_query() {
    // Arrange
    let _environment = EnvironmentGuard::set("CASSIE_ROOT_PASSWORD", "route-password");
    use_local_storage();
    let path = data_dir("pgwire-simple");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        seed_transport_boundary_docs(&cassie);
        let (addr, server) = spawn_pgwire_boundary_server(&cassie).await;

        // Act
        run_pgwire_boundary_query(addr).await;

        // Assert
        let metrics = cassie.metrics();
        assert_pgwire_boundary_metrics(&metrics);

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_rest_blocking_route_metrics_for_non_public_routes() {
    // Arrange
    let _environment = EnvironmentGuard::set("CASSIE_ROOT_PASSWORD", "route-password");
    use_local_storage();
    let path = data_dir("rest-route");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::rest::router::run(addr.to_string(), cassie.clone()));

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let client = reqwest::Client::new();
        let admin_cookie = client
            .post(format!("http://{addr}/api/v1/auth/login"))
            .json(&serde_json::json!({
                "username": "root",
                "password": "route-password"
            }))
            .send()
            .await
            .expect("login request")
            .headers()
            .get("set-cookie")
            .expect("session cookie")
            .to_str()
            .expect("session cookie value")
            .split(';')
            .next()
            .expect("session cookie pair")
            .to_string();
        let before = cassie.metrics();
        let before_started =
            read_boundary_counter(&before, "rest", "blocking_started_total", "rest_route");
        let before_completed =
            read_boundary_counter(&before, "rest", "blocking_completed_total", "rest_route");

        // Act
        let create_payload = serde_json::json!({
            "name": "boundary_rest_docs",
            "fields": [{"name": "title", "type": "text"}]
        });
        let create = client
            .post(format!("http://{addr}/api/v1/collections"))
            .header("content-type", "application/json")
            .header("cookie", &admin_cookie)
            .body(create_payload.to_string())
            .send()
            .await
            .expect("create request");
        assert_eq!(create.status(), reqwest::StatusCode::OK);

        let list = client
            .get(format!("http://{addr}/api/v1/collections"))
            .header("cookie", &admin_cookie)
            .send()
            .await
            .expect("list request");
        assert_eq!(list.status(), reqwest::StatusCode::OK);

        // Assert
        let metrics = cassie.metrics();
        let started =
            read_boundary_counter(&metrics, "rest", "blocking_started_total", "rest_route");
        let completed =
            read_boundary_counter(&metrics, "rest", "blocking_completed_total", "rest_route");
        let errors = read_boundary_counter(&metrics, "rest", "blocking_error_total", "rest_route");
        let join_failed =
            read_boundary_counter(&metrics, "rest", "blocking_join_failed_total", "rest_route");
        let has_latency = metrics["rest"]["blocking_elapsed_ms_total"]
            .get("rest_route")
            .is_some();
        let requests = metrics["rest"]["requests_total"]
            .as_u64()
            .unwrap_or_default();

        assert!(
            requests >= 2,
            "non-public route requests should be recorded"
        );
        assert_eq!(
            started - before_started,
            2,
            "route calls should use boundary helper"
        );
        assert_eq!(
            completed - before_completed,
            2,
            "route calls should complete through boundary helper"
        );
        assert_eq!(errors, 0, "successful route calls should not error");
        assert_eq!(
            join_failed, 0,
            "blocking join should not fail for in-memory execution"
        );
        assert!(has_latency, "elapsed metric should be present");

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
