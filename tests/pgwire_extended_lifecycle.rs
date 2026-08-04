#![allow(unused_imports, dead_code)]

#[path = "support/pgwire.rs"]
mod pgwire_support;

use std::net::SocketAddr;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use pgwire_support::{
    bind_frame, cancel_request_frame, data_dir, describe_statement_frame, execute_frame,
    parse_data_row, parse_error_fields, parse_frame, parse_parameter_description,
    parse_row_description, read_until_ready, read_wire_frame, startup_frame, sync_frame,
    use_local_storage,
};

type WireFrame = (u8, Vec<u8>);
type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

fn close_frame(target: u8, name: &str) -> Vec<u8> {
    match target {
        b'S' => pgwire_support::close_statement_frame(name),
        b'P' => pgwire_support::close_portal_frame(name),
        _ => panic!("unsupported close target: {target}"),
    }
}

fn frontend_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.push(tag);
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("frontend payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

fn read_cstring(payload: &[u8], cursor: &mut usize) -> String {
    let tail = payload
        .get(*cursor..)
        .expect("cursor should be inside payload");
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .expect("cstring should be null terminated");
    let value = std::str::from_utf8(&tail[..end]).expect("cstring should be utf-8");
    *cursor += end + 1;
    value.to_string()
}

fn read_i16(payload: &[u8], cursor: &mut usize) -> i16 {
    let start = *cursor;
    let end = start + 2;
    let bytes: [u8; 2] = payload[start..end].try_into().expect("i16 payload");
    *cursor = end;
    i16::from_be_bytes(bytes)
}

fn read_i32(payload: &[u8], cursor: &mut usize) -> i32 {
    let start = *cursor;
    let end = start + 4;
    let bytes: [u8; 4] = payload[start..end].try_into().expect("i32 payload");
    *cursor = end;
    i32::from_be_bytes(bytes)
}

fn seed_close_cascade_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "extended_query_close_docs");
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

async fn spawn_pgwire_server(cassie: &Cassie) -> (SocketAddr, PgwireServer) {
    let config = CassieRuntimeConfig::from_env().expect("runtime config");
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
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, server)
}

async fn start_pgwire_session(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(writer, &startup_frame("root", "postgres"))
        .await
        .expect("write startup");
    let auth = read_wire_frame(reader).await;
    assert_eq!(auth.0, b'R', "startup should return an auth response");
    if auth.1 == 3_i32.to_be_bytes() {
        tokio::io::AsyncWriteExt::write_all(writer, &frontend_frame(b'p', b"postgres\0"))
            .await
            .expect("write password");
        tokio::io::AsyncWriteExt::flush(writer)
            .await
            .expect("flush password");
        let auth_ok = read_wire_frame(reader).await;
        assert_eq!(auth_ok, (b'R', 0_i32.to_be_bytes().to_vec()));
    }
    let startup_ready = read_until_ready(reader).await;
    assert_eq!(startup_ready, vec![b'I']);
}

async fn write_close_cascade_batch(writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &parse_frame(
            "stmt_close_cascade",
            "SELECT title FROM extended_query_close_docs WHERE title = $1 ORDER BY title",
        ),
    )
    .await
    .expect("write parse");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &bind_frame("portal_close_cascade", "stmt_close_cascade", &["alpha"]),
    )
    .await
    .expect("write bind");
    tokio::io::AsyncWriteExt::write_all(writer, &close_frame(b'S', "stmt_close_cascade"))
        .await
        .expect("write statement close");
    tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_close_cascade"))
        .await
        .expect("write execute");
    tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
        .await
        .expect("write sync");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush close batch");
}

async fn read_ready_frames(reader: &mut PgwireReader<'_>) -> Vec<WireFrame> {
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

fn assert_close_cascade_frames(frames: &[WireFrame], cassie: &Cassie) {
    assert_eq!(
        frames.len(),
        5,
        "statement close should remove dependent portals before execute"
    );
    assert_eq!(frames[0].0, b'1');
    assert_eq!(frames[1].0, b'2');
    assert_eq!(frames[2].0, b'3');
    assert_eq!(frames[3].0, b'E');
    assert_eq!(frames[4].0, b'Z');
    assert_eq!(frames[4].1, vec![b'I']);

    let error_fields = parse_error_fields(&frames[3].1);
    assert!(
        error_fields
            .iter()
            .any(|(field, value)| *field == 'M' && value.contains("portal")),
        "execute after statement close should fail because the portal was removed"
    );
    assert!(
        error_fields
            .iter()
            .any(|(field, value)| *field == 'M' && value.contains("not bound")),
        "execute after statement close should mention the missing portal"
    );
    let metrics = cassie.metrics();
    assert_eq!(metrics["pgwire"]["prepared_statements"].as_u64(), Some(0));
    assert_eq!(metrics["pgwire"]["portals"].as_u64(), Some(0));
}

#[test]
fn should_close_statement_cascade_referenced_portals_before_reuse() {
    // Arrange
    use_local_storage();
    let path = data_dir("close_cascade");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = CassieRuntimeConfig::from_env().expect("runtime config");
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_close_cascade_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        write_close_cascade_batch(&mut write_half).await;
        let frames = read_ready_frames(&mut reader).await;

        // Assert
        assert_close_cascade_frames(&frames, &cassie);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
