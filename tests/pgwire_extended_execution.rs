#![allow(unused_imports, dead_code)]
use std::net::SocketAddr;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

type WireFrame = (u8, Vec<u8>);
type PgwireReader<'a> = tokio::io::BufReader<tokio::net::tcp::ReadHalf<'a>>;
type PgwireWriter<'a> = tokio::net::tcp::WriteHalf<'a>;
type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;

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

fn seed_extended_query_collection(cassie: &Cassie) {
    let collection = "extended_query_docs";
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
}

async fn spawn_pgwire_server(cassie: &Cassie) -> (SocketAddr, PgwireServer) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
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
    tokio::time::sleep(Duration::from_millis(50)).await;
    (addr, server)
}

async fn start_pgwire_session(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(writer, &startup_frame("postgres", "testdb"))
        .await
        .expect("write startup");
    let auth = read_wire_frame(reader).await;
    assert_eq!(auth.0, b'R', "startup should return an auth response");
    let startup_ready = read_until_ready(reader).await;
    assert_eq!(startup_ready, vec![b'I']);
    assert_eq!(
        i32::from_be_bytes(auth.1[0..4].try_into().expect("auth payload")),
        0,
        "passwordless auth should succeed"
    );
}

async fn write_extended_lifecycle_batch(writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &parse_frame(
            "stmt_extended_lifecycle",
            "SELECT title FROM extended_query_docs WHERE title = $1 ORDER BY title",
        ),
    )
    .await
    .expect("write parse");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &describe_statement_frame("stmt_extended_lifecycle"),
    )
    .await
    .expect("write describe");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &bind_frame(
            "portal_extended_lifecycle",
            "stmt_extended_lifecycle",
            &["alpha"],
        ),
    )
    .await
    .expect("write bind");
    tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_extended_lifecycle"))
        .await
        .expect("write execute");
    tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
        .await
        .expect("write sync");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush frames");
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

fn assert_extended_lifecycle_frames(frames: &[WireFrame]) {
    assert_eq!(
        frames.len(),
        7,
        "extended query should return seven backend frames"
    );
    assert_eq!(frames[0].0, b'1', "parse should complete first");
    assert_eq!(
        frames[1].0, b't',
        "describe should return parameter metadata first"
    );
    assert_eq!(frames[2].0, b'T', "describe should return row metadata");
    assert_eq!(frames[3].0, b'2', "bind should complete after describe");
    assert_eq!(frames[4].0, b'D', "execute should return a data row");
    assert_eq!(
        frames[5].0, b'C',
        "execute should end with command complete"
    );
    assert_eq!(frames[6].0, b'Z', "sync should finish with ready-for-query");

    let parameters = parse_parameter_description(&frames[1].1);
    assert_eq!(parameters, vec![25]);

    let fields = parse_row_description(&frames[2].1);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].0, "title");
    assert_eq!(fields[0].3, 25, "text columns should use the text OID");

    let values = parse_data_row(&frames[4].1);
    assert_eq!(values, vec![Some("alpha".to_string())]);

    let mut command_cursor = 0usize;
    let command = read_cstring(&frames[5].1, &mut command_cursor);
    assert!(
        command.starts_with("SELECT"),
        "command completion should identify the select command"
    );
    assert_eq!(frames[6].1, vec![b'I']);
}

#[test]
fn should_execute_binary_extended_query_lifecycle_return_backend_frames() {
    // Arrange
    with_fallback();
    let path = data_dir("lifecycle");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_extended_query_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        write_extended_lifecycle_batch(&mut write_half).await;
        let frames = read_ready_frames(&mut reader).await;

        // Assert
        assert_extended_lifecycle_frames(&frames);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
