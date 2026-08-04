#![allow(unused_imports, dead_code)]
use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};

#[path = "support/pgwire.rs"]
mod pgwire_support;

use pgwire_support::{
    bind_frame, cancel_request_frame, data_dir, describe_statement_frame, execute_frame,
    parse_data_row, parse_error_fields, parse_frame, parse_parameter_description,
    read_frames_until_ready, read_until_ready, read_wire_frame, startup_frame, sync_frame,
    use_local_storage,
};

fn password_frame(password: &str) -> Vec<u8> {
    pgwire_support::password_message(password)
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

fn parse_row_description(payload: &[u8]) -> Vec<(String, i32, i16, i32, i16)> {
    let mut cursor = 0usize;
    let field_count = read_i16(payload, &mut cursor);
    let mut fields = Vec::new();

    for _ in 0..field_count {
        let name = read_cstring(payload, &mut cursor);
        let table_oid = read_i32(payload, &mut cursor);
        let _attr_num = read_i16(payload, &mut cursor);
        let type_oid = read_i32(payload, &mut cursor);
        let type_size = read_i16(payload, &mut cursor);
        let _type_mod = read_i32(payload, &mut cursor);
        let format_code = read_i16(payload, &mut cursor);
        fields.push((name, table_oid, type_size, type_oid, format_code));
    }

    fields
}

fn score_schema() -> Schema {
    Schema {
        fields: vec![FieldSchema {
            name: "score".to_string(),
            data_type: DataType::Int,
            nullable: true,
        }],
    }
}

fn seed_score_collection(cassie: &Cassie, collection: &str) {
    let collection = canonical_relation_name("postgres", "public", collection);
    let schema = score_schema();
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .unwrap();
    cassie.register_collection(&collection, schema);
    for (id, score) in [("doc-1", 1), ("doc-2", 2)] {
        cassie
            .midge
            .put_document(
                &collection,
                Some(id.to_string()),
                serde_json::json!({"score": score}),
            )
            .unwrap();
    }
}

async fn spawn_pgwire_server(
    cassie: &Cassie,
) -> (
    std::net::SocketAddr,
    tokio::task::JoinHandle<Result<(), cassie::CassieError>>,
) {
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
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, server)
}

async fn connect_authenticated_pgwire(
    addr: std::net::SocketAddr,
) -> (
    tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let socket = tokio::net::TcpStream::connect(addr)
        .await
        .expect("connect pgwire");
    let (read_half, mut write_half) = socket.into_split();
    let mut reader = tokio::io::BufReader::new(read_half);
    tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("root", "postgres"))
        .await
        .expect("write startup");
    let auth = read_wire_frame(&mut reader).await;
    assert_eq!(auth.0, b'R', "startup should return an auth response");
    assert_eq!(
        i32::from_be_bytes(auth.1[0..4].try_into().expect("auth payload")),
        3,
        "startup should request a cleartext password"
    );
    tokio::io::AsyncWriteExt::write_all(&mut write_half, &password_frame("postgres"))
        .await
        .expect("write password");
    tokio::io::AsyncWriteExt::flush(&mut write_half)
        .await
        .expect("flush password");
    let auth_ok = read_wire_frame(&mut reader).await;
    assert_eq!(auth_ok.0, b'R', "password should return an auth response");
    assert_eq!(
        i32::from_be_bytes(auth_ok.1[0..4].try_into().expect("auth payload")),
        0,
        "password auth should succeed"
    );
    let startup_ready = read_until_ready(&mut reader).await;
    assert_eq!(startup_ready, vec![b'I']);
    (reader, write_half)
}

async fn execute_reused_statement(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    statement_name: &str,
    sql: &str,
    portals: [(&str, &str); 2],
) {
    tokio::io::AsyncWriteExt::write_all(writer, &parse_frame(statement_name, sql))
        .await
        .expect("write parse");
    for (portal_name, param) in portals {
        tokio::io::AsyncWriteExt::write_all(
            writer,
            &bind_frame(portal_name, statement_name, &[param]),
        )
        .await
        .expect("write bind");
        tokio::io::AsyncWriteExt::write_all(writer, &execute_frame(portal_name))
            .await
            .expect("write execute");
    }
    tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
        .await
        .expect("write sync");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush frames");
}

fn assert_reused_statement_frames(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(
        frames.len(),
        10,
        "reused prepared statements should return ten frames"
    );
    assert_eq!(frames[0].0, b'1', "parse should complete first");
    assert_eq!(frames[1].0, b'2', "first bind should complete");
    assert_eq!(frames[2].0, b'T', "first execute should describe rows");
    assert_eq!(frames[3].0, b'D', "first execute should return a data row");
    assert_eq!(
        frames[4].0, b'C',
        "first execute should finish with command complete"
    );
    assert_eq!(
        frames[5].0, b'2',
        "second bind should reuse the prepared statement"
    );
    assert_eq!(frames[6].0, b'T', "second execute should describe rows");
    assert_eq!(frames[7].0, b'D', "second execute should return a data row");
    assert_eq!(
        frames[8].0, b'C',
        "second execute should finish with command complete"
    );
    assert_eq!(frames[9].0, b'Z', "sync should finish with ready-for-query");
}

async fn shutdown_pgwire_server(server: tokio::task::JoinHandle<Result<(), cassie::CassieError>>) {
    server.abort();
    let _ = server.await;
}

#[test]
fn should_reuse_prepared_statement_for_binary_extended_query_bindings() {
    // Arrange
    use_local_storage();
    let path = data_dir("reuse");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_score_collection(&cassie, "extended_query_numbers");
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let (mut reader, mut write_half) = connect_authenticated_pgwire(addr).await;

        // Act
        execute_reused_statement(
            &mut write_half,
            "stmt_extended_reuse",
            "SELECT score FROM extended_query_numbers WHERE score = $1 ORDER BY score",
            [("portal_one", "1"), ("portal_two", "2")],
        )
        .await;
        let frames = read_frames_until_ready(&mut reader).await;

        // Assert
        assert_reused_statement_frames(&frames);
        let first_values = parse_data_row(&frames[3].1);
        let second_values = parse_data_row(&frames[7].1);
        assert_eq!(first_values, vec![Some("1".to_string())]);
        assert_eq!(second_values, vec![Some("2".to_string())]);

        drop(write_half);
        shutdown_pgwire_server(server).await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_parse_prepared_statement_once_across_repeated_extended_executes() {
    // Arrange
    use_local_storage();
    let path = data_dir("parse_once");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_score_collection(&cassie, "extended_query_parse_once");
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let (mut reader, mut write_half) = connect_authenticated_pgwire(addr).await;

        // Act
        execute_reused_statement(
            &mut write_half,
            "stmt_extended_parse_once",
            "SELECT score FROM extended_query_parse_once WHERE score = $1 ORDER BY score",
            [
                ("portal_parse_once_one", "1"),
                ("portal_parse_once_two", "2"),
            ],
        )
        .await;
        let _ = read_frames_until_ready(&mut reader).await;
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(metrics["runtime"]["sql_parse_total"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["misses"].as_u64(), Some(1));
        assert_eq!(metrics["plan_cache"]["hits"].as_u64(), Some(1));

        drop(write_half);
        shutdown_pgwire_server(server).await;
        let _ = std::fs::remove_dir_all(path);
    });
}
