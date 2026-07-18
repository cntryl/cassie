#![allow(unused_imports, dead_code)]
use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-pgwire-extended-query-{}-{}",
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

fn cancel_request_frame(process_id: i32, secret_key: i32) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.extend_from_slice(&16_i32.to_be_bytes());
    frame.extend_from_slice(&80_877_102_i32.to_be_bytes());
    frame.extend_from_slice(&process_id.to_be_bytes());
    frame.extend_from_slice(&secret_key.to_be_bytes());
    frame
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

fn password_frame(password: &str) -> Vec<u8> {
    let mut payload = password.as_bytes().to_vec();
    payload.push(0);
    frontend_frame(b'p', &payload)
}

fn parse_frame(statement_name: &str, sql: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(sql.as_bytes());
    payload.push(0);
    payload.extend_from_slice(&0_i16.to_be_bytes());

    let mut frame = Vec::new();
    frame.push(b'P');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("parse payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn bind_frame(portal_name: &str, statement_name: &str, params: &[&str]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(portal_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&0_i16.to_be_bytes());
    payload.extend_from_slice(
        &i16::try_from(params.len())
            .expect("parameter count must fit into i16")
            .to_be_bytes(),
    );
    for param in params {
        payload.extend_from_slice(
            &i32::try_from(param.len())
                .expect("parameter length must fit into i32")
                .to_be_bytes(),
        );
        payload.extend_from_slice(param.as_bytes());
    }
    payload.extend_from_slice(&0_i16.to_be_bytes());

    let mut frame = Vec::new();
    frame.push(b'B');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("bind payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn describe_statement_frame(statement_name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(b'S');
    payload.extend_from_slice(statement_name.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'D');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("describe payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn execute_frame(portal_name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(portal_name.as_bytes());
    payload.push(0);
    payload.extend_from_slice(&0_i32.to_be_bytes());

    let mut frame = Vec::new();
    frame.push(b'E');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("execute payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn close_frame(target: u8, name: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(target);
    payload.extend_from_slice(name.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'C');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("close payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn sync_frame() -> Vec<u8> {
    let mut frame = Vec::new();
    frame.push(b'S');
    frame.extend_from_slice(&4_i32.to_be_bytes());
    frame
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<impl tokio::io::AsyncRead + Unpin>,
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
    reader: &mut tokio::io::BufReader<impl tokio::io::AsyncRead + Unpin>,
) -> Vec<u8> {
    loop {
        let frame = read_wire_frame(reader).await;
        if frame.0 == b'Z' {
            return frame.1;
        }
    }
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

fn parse_parameter_description(payload: &[u8]) -> Vec<i32> {
    let mut cursor = 0usize;
    let parameter_count = read_i16(payload, &mut cursor);
    let mut parameters = Vec::new();

    for _ in 0..parameter_count {
        parameters.push(read_i32(payload, &mut cursor));
    }

    parameters
}

fn parse_data_row(payload: &[u8]) -> Vec<Option<String>> {
    let mut cursor = 0usize;
    let field_count = read_i16(payload, &mut cursor);
    let mut values = Vec::new();

    for _ in 0..field_count {
        let len = read_i32(payload, &mut cursor);
        if len < 0 {
            values.push(None);
            continue;
        }
        let len = usize::try_from(len).expect("payload length should fit usize");
        let end = cursor + len;
        let text = std::str::from_utf8(&payload[cursor..end]).expect("data row should be utf-8");
        cursor = end;
        values.push(Some(text.to_string()));
    }

    values
}

fn parse_error_fields(payload: &[u8]) -> Vec<(char, String)> {
    let mut cursor = 0usize;
    let mut fields = Vec::new();

    while cursor < payload.len() {
        let field_type = payload[cursor];
        cursor += 1;
        if field_type == 0 {
            break;
        }
        let value = read_cstring(payload, &mut cursor);
        fields.push((char::from(field_type), value));
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
    tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("postgres", "postgres"))
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

async fn read_frames_until_ready(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Vec<(u8, Vec<u8>)> {
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
    with_fallback();
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
    with_fallback();
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
