use std::net::SocketAddr;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

type WireFrame = (u8, Vec<u8>);
type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-pgwire-metrics-{}-{}",
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
) -> (u8, i32, Vec<u8>) {
    let mut header = [0u8; 5];
    tokio::io::AsyncReadExt::read_exact(reader, &mut header)
        .await
        .expect("read auth frame header");

    let tag = header[0];
    let len = i32::from_be_bytes(header[1..].try_into().expect("auth frame length"));
    let mut payload =
        vec![0u8; usize::try_from(len - 4).expect("non-negative auth payload length")];
    tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
        .await
        .expect("read auth frame payload");

    (tag, len, payload)
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

async fn read_until_ready(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Vec<u8> {
    loop {
        let frame = read_wire_frame(reader).await;
        if frame.0 == b'Z' {
            return frame.1;
        }
    }
}

fn seed_pgwire_metrics_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "pgwire_metrics_docs");
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
    cassie.catalog.register_collection(
        &collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    cassie
        .midge
        .put_document(
            &collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .unwrap();
}

async fn spawn_pgwire_metrics_server(
    cassie: &Cassie,
    config: CassieRuntimeConfig,
) -> (SocketAddr, PgwireServer) {
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

async fn run_pgwire_metrics_query(addr: SocketAddr) -> Vec<WireFrame> {
    let mut socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect pgwire");
    let (read_half, mut write_half) = socket.split();
    let mut reader = tokio::io::BufReader::new(read_half);

    let startup = startup_frame("postgres", "postgres");
    tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup)
        .await
        .expect("startup write");

    let auth = read_auth_frame(&mut reader).await;
    assert_eq!(
        auth.0, b'R',
        "startup should return an authentication frame"
    );
    let startup_ready = read_until_ready(&mut reader).await;
    assert_eq!(startup_ready, vec![b'I']);

    tokio::io::AsyncWriteExt::write_all(
        &mut write_half,
        &simple_query_frame("SELECT title FROM pgwire_metrics_docs ORDER BY title"),
    )
    .await
    .expect("query write");
    tokio::io::AsyncWriteExt::flush(&mut write_half)
        .await
        .expect("flush");

    read_ready_frames(&mut reader).await
}

async fn read_ready_frames(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Vec<WireFrame> {
    let mut frames = Vec::new();
    loop {
        let frame = read_wire_frame(reader).await;
        let tag = frame.0;
        frames.push(frame);
        if tag == b'Z' {
            return frames;
        }
    }
}

fn assert_pgwire_query_frames(frames: &[WireFrame]) {
    assert!(
        frames.iter().any(|frame| frame.0 == b'T'),
        "pgwire query should return a row description frame"
    );
    assert!(
        frames.iter().any(|frame| frame.0 == b'D'),
        "pgwire query should return a data row frame"
    );
    assert!(frames.iter().any(|frame| frame.0 == b'C'));
    assert!(frames.iter().any(|frame| frame.0 == b'Z'));
}

fn assert_pgwire_metrics(metrics: &serde_json::Value) {
    assert_eq!(
        metrics["pgwire"]["sessions_started_total"].as_u64(),
        Some(1)
    );
    assert_eq!(metrics["pgwire"]["auth_ok_total"].as_u64(), Some(1));
    assert_eq!(metrics["pgwire"]["simple_queries_total"].as_u64(), Some(1));
    assert_eq!(metrics["pgwire"]["active_sessions"].as_u64(), Some(0));
    assert_eq!(metrics["pgwire"]["prepared_statements"].as_u64(), Some(0));
}

#[test]
fn should_record_pgwire_connection_metrics() {
    // Arrange
    with_fallback();
    let path = data_dir("session_messages");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        seed_pgwire_metrics_collection(&cassie);
        let (addr, server) = spawn_pgwire_metrics_server(&cassie, config).await;

        // Act
        let frames = run_pgwire_metrics_query(addr).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let metrics = cassie.metrics();

        // Assert
        assert_pgwire_query_frames(&frames);
        assert_pgwire_metrics(&metrics);

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
