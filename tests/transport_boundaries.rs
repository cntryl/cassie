use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-transport-boundaries-{}-{}",
        label,
        Uuid::new_v4()
    ));
    path.to_string_lossy().to_string()
}

fn startup_frame(user: &str, database: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0");
    payload.extend_from_slice(user.as_bytes());
    payload.push(0);
    payload.extend_from_slice(b"database\0");
    payload.extend_from_slice(database.as_bytes());
    payload.push(0);
    payload.push(0);

    let mut frame = Vec::new();
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("startup payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn simple_query_frame(sql: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(sql.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'Q');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("simple query payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn read_auth_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, Vec<u8>) {
    let mut header = [0u8; 5];
    tokio::io::AsyncReadExt::read_exact(reader, &mut header)
        .await
        .expect("read auth frame header");

    let len = i32::from_be_bytes(header[1..].try_into().expect("auth frame length"));
    let mut payload =
        vec![0u8; usize::try_from(len - 4).expect("non-negative auth payload length")];
    tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
        .await
        .expect("read auth frame payload");

    (header[0], payload)
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, Vec<u8>) {
    let mut tag = [0u8; 1];
    tokio::io::AsyncReadExt::read_exact(reader, &mut tag)
        .await
        .expect("read frame tag");

    let mut len = [0u8; 4];
    tokio::io::AsyncReadExt::read_exact(reader, &mut len)
        .await
        .expect("read frame length");
    let len = i32::from_be_bytes(len);
    let mut payload = vec![0u8; usize::try_from(len - 4).expect("non-negative payload length")];
    if !payload.is_empty() {
        tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
            .await
            .expect("read frame payload");
    }

    (tag[0], payload)
}

fn read_boundary_counter(
    metrics: &serde_json::Value,
    interface: &str,
    kind: &str,
    op: &str,
) -> u64 {
    metrics[interface][kind][op].as_u64().unwrap_or_default()
}

#[test]
fn should_record_pgwire_blocking_boundary_metrics_for_simple_query() {
    // Arrange
    with_fallback();
    let path = data_dir("pgwire-simple");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        std::env::set_var("CASSIE_ADMIN_PASSWORD", "route-password");
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "transport_boundary_docs";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(collection, schema);
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();

        let mut config = CassieRuntimeConfig::from_env();
        config.password.clear();
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

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("postgres", "testdb"))
            .await
            .expect("startup write");
        let _auth_frame = read_auth_frame(&mut reader).await;
        let _ready = read_wire_frame(&mut reader).await;

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

        // Assert
        let metrics = cassie.metrics();
        let started = read_boundary_counter(
            &metrics,
            "pgwire",
            "blocking_started_total",
            "pgwire_simple_query",
        );
        let completed = read_boundary_counter(
            &metrics,
            "pgwire",
            "blocking_completed_total",
            "pgwire_simple_query",
        );
        let errors = read_boundary_counter(
            &metrics,
            "pgwire",
            "blocking_error_total",
            "pgwire_simple_query",
        );
        let join_failed = read_boundary_counter(
            &metrics,
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

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_record_rest_blocking_route_metrics_for_non_public_routes() {
    // Arrange
    with_fallback();
    let path = data_dir("rest-route");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        std::env::set_var("CASSIE_ADMIN_PASSWORD", "route-password");
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
            .post(format!("http://{addr}/v1/collections"))
            .header("content-type", "application/json")
            .header("authorization", "Bearer postgres:route-password")
            .body(create_payload.to_string())
            .send()
            .await
            .expect("create request");
        assert_eq!(create.status(), reqwest::StatusCode::OK);

        let list = client
            .get(format!("http://{addr}/v1/collections"))
            .header("authorization", "Bearer postgres:route-password")
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

fn assert_offloaded_calls(
    file: &str,
    source: &str,
    helper: &str,
    forbidden: &[&str],
    context_lines: usize,
) {
    let lines: Vec<&str> = source.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        for forbidden_call in forbidden {
            if !line.contains(forbidden_call) {
                continue;
            }

            let start = index.saturating_sub(context_lines);
            let allowed = (start..=index).any(|candidate| lines[candidate].contains(helper));
            assert!(
                allowed,
                "found direct blocking call '{forbidden_call}' in {file} outside {helper}: {line}"
            );
        }
    }
}

#[test]
fn should_forbid_direct_async_transport_calls_without_blocking_helpers() {
    // Arrange
    let pgwire_source = include_str!("../src/pgwire/connection.rs");
    let rest_source = include_str!("../src/rest/router.rs");

    // Act
    assert_offloaded_calls(
        "src/pgwire/connection.rs",
        pgwire_source,
        "run_pgwire_blocking",
        &[
            "cassie.authenticate_role",
            "cassie.execute_sql",
            "cassie.describe_parsed_statement",
            "cassie.execute_preparsed_statement_with_mode",
        ],
        10,
    );
    assert_offloaded_calls(
        "src/rest/router.rs",
        rest_source,
        "run_rest_blocking",
        &[
            "crate::rest::collections::list",
            "crate::rest::collections::create",
            "crate::rest::documents::create",
            "crate::rest::documents::get",
            "crate::rest::documents::delete",
            "crate::rest::indexes::create",
            "crate::rest::search::vector_search",
            "cassie.authenticate_role",
            "cassie.lookup_role",
        ],
        10,
    );
    // Assert
}
