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

fn password_frame(password: &str) -> Vec<u8> {
    pgwire_support::password_message(password)
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

fn close_frame(target: u8, name: &str) -> Vec<u8> {
    match target {
        b'S' => pgwire_support::close_statement_frame(name),
        b'P' => pgwire_support::close_portal_frame(name),
        _ => panic!("unsupported close target: {target}"),
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

fn seed_recovery_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "extended_query_recovery_docs");
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

async fn start_pgwire_session(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(writer, &startup_frame("root", "postgres"))
        .await
        .expect("write startup");
    let auth = read_wire_frame(reader).await;
    assert_eq!(auth.0, b'R', "startup should return an auth response");
    let mut cursor = 0;
    assert_eq!(read_i32(&auth.1, &mut cursor), 3);
    tokio::io::AsyncWriteExt::write_all(writer, &password_frame("postgres"))
        .await
        .expect("write password");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush password");
    let auth_ok = read_wire_frame(reader).await;
    assert_eq!(auth_ok.0, b'R', "password should return an auth response");
    let mut cursor = 0;
    assert_eq!(read_i32(&auth_ok.1, &mut cursor), 0);
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
    let tags = frames
        .iter()
        .map(|frame| char::from(frame.0))
        .collect::<String>();
    assert_eq!(
        frames.len(),
        6,
        "recovered query should execute normally, tags={tags}"
    );
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
    use_local_storage();
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
    use_local_storage();
    let path = data_dir("copy_data");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
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
        start_pgwire_session(&mut reader, &mut write_half).await;

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
    use_local_storage();
    let path = data_dir("recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
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
    use_local_storage();
    let path = data_dir("copy_unsupported");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
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
        start_pgwire_session(&mut reader, &mut write_half).await;

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
