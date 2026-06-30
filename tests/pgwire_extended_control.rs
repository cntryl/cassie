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

fn seed_recovery_collection(cassie: &Cassie) {
    let collection = "extended_query_recovery_docs";
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
}

async fn write_parse_error_recovery_batch(writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &parse_frame("stmt_recovery_error", "SELECT * FROM"),
    )
    .await
    .expect("write invalid parse");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &parse_frame(
            "stmt_recovery_valid",
            "SELECT title FROM extended_query_recovery_docs WHERE title = $1 ORDER BY title",
        ),
    )
    .await
    .expect("write ignored parse");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &bind_frame("portal_recovery", "stmt_recovery_valid", &["alpha"]),
    )
    .await
    .expect("write ignored bind");
    tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_recovery"))
        .await
        .expect("write ignored execute");
    tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
        .await
        .expect("write sync");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush recovery batch");
}

fn assert_parse_error_recovery(error: &WireFrame, ready: &WireFrame) {
    assert_eq!(error.0, b'E', "parse failure should return an error frame");
    assert_eq!(
        ready.0, b'Z',
        "sync after a parse failure should restore ready-for-query"
    );
    assert_eq!(
        parse_error_fields(&error.1)
            .iter()
            .find(|(field, _)| *field == 'C')
            .map(|(_, value)| value.as_str()),
        Some("42601"),
        "parse failure should be reported as a syntax error"
    );
    assert_eq!(ready.1, vec![b'I']);
}

async fn write_recovered_query_batch(writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &parse_frame(
            "stmt_recovery_valid",
            "SELECT title FROM extended_query_recovery_docs WHERE title = $1 ORDER BY title",
        ),
    )
    .await
    .expect("write recovery parse");
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &bind_frame("portal_recovery", "stmt_recovery_valid", &["alpha"]),
    )
    .await
    .expect("write recovery bind");
    tokio::io::AsyncWriteExt::write_all(writer, &execute_frame("portal_recovery"))
        .await
        .expect("write recovery execute");
    tokio::io::AsyncWriteExt::write_all(writer, &sync_frame())
        .await
        .expect("write recovery sync");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush recovery follow-up");
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

fn assert_recovered_query_frames(frames: &[WireFrame]) {
    assert_eq!(frames.len(), 6, "recovered query should execute normally");
    assert_eq!(frames[0].0, b'1');
    assert_eq!(frames[1].0, b'2');
    assert_eq!(frames[2].0, b'T');
    assert_eq!(frames[3].0, b'D');
    assert_eq!(frames[4].0, b'C');
    assert_eq!(frames[5].0, b'Z');
    assert_eq!(frames[5].1, vec![b'I']);

    let values = parse_data_row(&frames[3].1);
    assert_eq!(values, vec![Some("alpha".to_string())]);
}

#[test]
fn should_close_connection_on_cancel_request_without_response() {
    // Arrange
    with_fallback();
    let path = data_dir("cancel");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

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

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &cancel_request_frame(11_223_344, 55_667_788),
        )
        .await
        .expect("write cancel request");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush cancel request");

        let mut buffer = [0u8; 1];
        let read = tokio::time::timeout(
            Duration::from_secs(1),
            tokio::io::AsyncReadExt::read(&mut reader, &mut buffer),
        )
        .await
        .expect("cancel request should close promptly")
        .expect("read cancel response");

        // Assert
        assert_eq!(read, 0, "cancel request should not produce a response");

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_copy_data_message_with_unsupported_error() {
    // Arrange
    with_fallback();
    let path = data_dir("copy_data");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

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

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("postgres", "testdb"))
            .await
            .expect("write startup");
        let auth = read_wire_frame(&mut reader).await;
        assert_eq!(auth.0, b'R', "startup should return an auth response");
        let startup_ready = read_until_ready(&mut reader).await;
        assert_eq!(startup_ready, vec![b'I']);

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &frontend_frame(b'd', b"copy payload"),
        )
        .await
        .expect("write copy data");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &sync_frame())
            .await
            .expect("write sync");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush copy data batch");

        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;

        // Assert
        assert_eq!(
            error.0, b'E',
            "copy data should be rejected with an error frame"
        );
        assert_eq!(ready.0, b'Z', "sync after copy rejection should recover");
        assert_eq!(ready.1, vec![b'I']);
        let error_fields = parse_error_fields(&error.1);
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("0A000"),
            "copy data should return an unsupported-feature SQLSTATE"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_extended_query_messages_until_sync_after_parse_error() {
    // Arrange
    with_fallback();
    let path = data_dir("recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        seed_recovery_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        write_parse_error_recovery_batch(&mut write_half).await;
        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;

        // Assert
        assert_parse_error_recovery(&error, &ready);
        write_recovered_query_batch(&mut write_half).await;
        let frames = read_ready_frames(&mut reader).await;
        assert_recovered_query_frames(&frames);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_unsupported_error_for_copy_statement() {
    // Arrange
    with_fallback();
    let path = data_dir("copy_unsupported");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

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

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("postgres", "testdb"))
            .await
            .expect("write startup");
        let auth = read_wire_frame(&mut reader).await;
        assert_eq!(auth.0, b'R', "startup should return an auth response");
        let startup_ready = read_until_ready(&mut reader).await;
        assert_eq!(startup_ready, vec![b'I']);

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &parse_frame("stmt_copy", "COPY extended_query_close_docs TO STDOUT"),
        )
        .await
        .expect("write copy parse");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &sync_frame())
            .await
            .expect("write sync");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush copy batch");

        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;

        // Assert
        assert_eq!(error.0, b'E', "copy should be rejected with an error frame");
        assert_eq!(ready.0, b'Z', "sync after copy rejection should recover");
        assert_eq!(ready.1, vec![b'I']);
        let error_fields = parse_error_fields(&error.1);
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("0A000"),
            "copy should return an unsupported-feature SQLSTATE"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
