use std::net::SocketAddr;
use std::time::Duration;

use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
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
        "cassie-pgwire-simple-query-{}-{}",
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

fn password_message(password: &str) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(password.as_bytes());
    payload.push(0);

    let mut frame = Vec::new();
    frame.push(b'p');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("password payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

fn copy_data_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.push(b'd');
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("copy payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(payload);
    frame
}

fn copy_done_frame() -> Vec<u8> {
    let mut frame = Vec::new();
    frame.push(b'c');
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

fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field, _)| *field == tag)
        .map(|(_, value)| value.as_str())
}

fn seed_copy_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "simple_copy_docs");
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: false,
            },
            FieldSchema {
                name: "score".to_string(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection(&collection, schema.clone())
        .unwrap();
    cassie.register_collection(&collection, schema);
}

fn seed_simple_query_collection(cassie: &Cassie) {
    let collection = canonical_relation_name("postgres", "public", "simple_query_docs");
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
    spawn_pgwire_server_with_config(
        cassie,
        CassieRuntimeConfig::from_env().expect("runtime config"),
    )
    .await
}

async fn spawn_pgwire_server_with_config(
    cassie: &Cassie,
    mut config: CassieRuntimeConfig,
) -> (SocketAddr, PgwireServer) {
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
    tokio::io::AsyncWriteExt::write_all(writer, &startup_frame("postgres", "postgres"))
        .await
        .expect("write startup");
    let (auth_tag, auth_payload) = read_wire_frame(reader).await;
    assert_eq!(auth_tag, b'R', "startup should return an auth response");
    let auth_status = i32::from_be_bytes(auth_payload[0..4].try_into().expect("auth status"));
    match auth_status {
        0 => {}
        3 => {
            tokio::io::AsyncWriteExt::write_all(writer, &password_message("postgres"))
                .await
                .expect("write password");
            tokio::io::AsyncWriteExt::flush(writer)
                .await
                .expect("flush password");
            let (auth_ok_tag, auth_ok_payload) = read_wire_frame(reader).await;
            assert_eq!(auth_ok_tag, b'R', "auth success should use auth response");
            assert_eq!(
                i32::from_be_bytes(auth_ok_payload[0..4].try_into().expect("auth ok status")),
                0,
                "cleartext auth should accept the configured password"
            );
        }
        other => panic!("unexpected auth status {other}"),
    }
    let startup_ready = read_until_ready(reader).await;
    assert_eq!(startup_ready, vec![b'I']);
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

async fn write_simple_query_and_read_frames(
    reader: &mut PgwireReader<'_>,
    writer: &mut PgwireWriter<'_>,
    sql: &str,
) -> Vec<WireFrame> {
    tokio::io::AsyncWriteExt::write_all(writer, &simple_query_frame(sql))
        .await
        .expect("write query");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush query");
    read_ready_frames(reader).await
}

async fn request_copy_from_stdin(reader: &mut PgwireReader<'_>, writer: &mut PgwireWriter<'_>) {
    tokio::io::AsyncWriteExt::write_all(
        writer,
        &simple_query_frame(
            "COPY simple_copy_docs (_id, title, score) FROM STDIN WITH (FORMAT csv, HEADER true)",
        ),
    )
    .await
    .expect("write copy query");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush copy query");

    let copy_in = read_wire_frame(reader).await;
    assert_eq!(copy_in.0, b'G', "copy should return CopyInResponse");
    assert_eq!(copy_in.1[0], 0, "COPY should use text format");
    assert_eq!(
        i16::from_be_bytes(copy_in.1[1..3].try_into().expect("copy column count")),
        3
    );
}

async fn send_copy_rows(
    reader: &mut PgwireReader<'_>,
    writer: &mut PgwireWriter<'_>,
) -> (WireFrame, WireFrame) {
    let copy_payload = b"_id,title,score\ncopy-1,alpha,7\ncopy-2,beta,9\n";
    tokio::io::AsyncWriteExt::write_all(writer, &copy_data_frame(copy_payload))
        .await
        .expect("write copy data");
    tokio::io::AsyncWriteExt::write_all(writer, &copy_done_frame())
        .await
        .expect("write copy done");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush copy data");

    let complete = read_wire_frame(reader).await;
    let ready = read_wire_frame(reader).await;
    (complete, ready)
}

fn assert_copy_complete_frames(complete: &WireFrame, ready: &WireFrame) {
    assert_eq!(
        complete.0,
        b'C',
        "copy should complete command: {:?}",
        parse_error_fields(&complete.1)
    );
    let mut command_cursor = 0usize;
    assert_eq!(read_cstring(&complete.1, &mut command_cursor), "COPY 2");
    assert_eq!(ready.0, b'Z');
    assert_eq!(ready.1, vec![b'I']);
}

fn assert_copy_select_frames(frames: &[WireFrame]) {
    assert_eq!(frames[0].0, b'T');
    assert_eq!(frames[1].0, b'D');
    assert_eq!(frames[2].0, b'D');
    assert_eq!(
        parse_data_row(&frames[1].1),
        vec![Some("alpha".to_string()), Some("7".to_string())]
    );
    assert_eq!(
        parse_data_row(&frames[2].1),
        vec![Some("beta".to_string()), Some("9".to_string())]
    );
    assert_eq!(frames[3].0, b'C');
    assert_eq!(frames[4].0, b'Z');
}

fn assert_simple_query_backend_frames(frames: &[WireFrame]) {
    assert_eq!(
        frames.len(),
        4,
        "simple query should return four backend frames"
    );
    assert_eq!(frames[0].0, b'T', "first frame should be row description");
    assert_eq!(frames[1].0, b'D', "second frame should be a data row");
    assert_eq!(frames[2].0, b'C', "third frame should be command complete");
    assert_eq!(frames[3].0, b'Z', "final frame should be ready for query");

    let fields = parse_row_description(&frames[0].1);
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].0, "title");
    assert_eq!(fields[0].3, 25, "text columns should use the text OID");

    let values = parse_data_row(&frames[1].1);
    assert_eq!(values, vec![Some("alpha".to_string())]);

    let mut command_cursor = 0usize;
    let command = read_cstring(&frames[2].1, &mut command_cursor);
    assert!(
        command.starts_with("SELECT"),
        "command completion should identify the select command"
    );
    assert_eq!(frames[3].1, vec![b'I']);
}

#[test]
fn should_copy_csv_from_stdin_rows() {
    // Arrange
    with_fallback();
    let path = data_dir("copy_stdin");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_copy_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        request_copy_from_stdin(&mut reader, &mut write_half).await;
        let (complete, ready) = send_copy_rows(&mut reader, &mut write_half).await;

        // Assert
        assert_copy_complete_frames(&complete, &ready);
        let select_frames = write_simple_query_and_read_frames(
            &mut reader,
            &mut write_half,
            "SELECT title, score FROM simple_copy_docs ORDER BY score",
        )
        .await;
        assert_copy_select_frames(&select_frames);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_binary_simple_query_return_backend_frames() {
    // Arrange
    with_fallback();
    let path = data_dir("success");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_simple_query_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        let frames = write_simple_query_and_read_frames(
            &mut reader,
            &mut write_half,
            "SELECT title FROM simple_query_docs ORDER BY title",
        )
        .await;

        // Assert
        assert_simple_query_backend_frames(&frames);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_row_description_for_empty_simple_query_result() {
    // Arrange
    with_fallback();
    let path = data_dir("empty_result");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();

        let collection = canonical_relation_name("postgres", "public", "simple_query_empty_docs");
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

        start_pgwire_session(&mut reader, &mut write_half).await;

        // Act
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame(
                "SELECT title FROM simple_query_empty_docs WHERE title = 'missing'",
            ),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");

        let mut frames = Vec::new();
        loop {
            let frame = read_wire_frame(&mut reader).await;
            let tag = frame.0;
            frames.push(frame);
            if tag == b'Z' {
                break;
            }
        }

        // Assert
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].0, b'T', "empty select should describe columns");
        assert_eq!(frames[1].0, b'C', "empty select should complete command");
        assert_eq!(frames[2].0, b'Z', "empty select should return ready");
        assert_eq!(parse_row_description(&frames[0].1)[0].0, "title");
        assert_eq!(frames[2].1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_recover_ready_after_simple_query_error() {
    // Arrange
    with_fallback();
    let path = data_dir("error");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
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
            &simple_query_frame("SELECT title FROM missing_simple_query_table"),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");

        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;

        // Assert
        assert_eq!(error.0, b'E', "query failure should return an error frame");
        assert_eq!(
            ready.0, b'Z',
            "query failure should still return ready-for-query"
        );
        let error_fields = parse_error_fields(&error.1);
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("42P01"),
            "missing table should use undefined table SQLSTATE"
        );
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 't')
                .map(|(_, value)| value.as_str()),
            Some("missing_simple_query_table"),
            "missing table should include table metadata"
        );
        assert!(
            error_fields.iter().any(|(field, value)| {
                *field == 'M' && value.contains("missing_simple_query_table")
            }),
            "error response should mention the missing table"
        );
        assert_eq!(ready.1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_retryable_storage_error_with_cannot_connect_now_sqlstate() {
    // Arrange
    with_fallback();
    let path = data_dir("retryable_storage");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password.clear();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        cassie::pgwire::connection::arm_next_pgwire_blocking_retryable_failure_for_test(
            &cassie,
            "pgwire blocking boundary test retryable failure",
        );
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT version()"),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");
        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;
        let error_fields = parse_error_fields(&error.1);

        // Assert
        assert_eq!(error.0, b'E');
        assert_eq!(ready.0, b'Z');
        assert_eq!(error_field(&error_fields, 'C'), Some("57P03"));
        assert_eq!(
            error_field(&error_fields, 'M'),
            Some("temporary storage unavailable: pgwire blocking boundary test retryable failure")
        );
        assert_eq!(ready.1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_deadline_exceeded_with_query_canceled_sqlstate() {
    // Arrange
    with_fallback();
    let path = data_dir("query_deadline");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_timeout_ms = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().expect("startup");
        seed_simple_query_collection(&cassie);
        let (addr, server) = spawn_pgwire_server_with_config(&cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        let sql = format!(
            "{}SELECT title FROM simple_query_docs",
            " ".repeat(1_000_000)
        );
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &simple_query_frame(&sql))
            .await
            .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");
        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;
        let error_fields = parse_error_fields(&error.1);

        // Assert
        assert_eq!(error.0, b'E');
        assert_eq!(ready.0, b'Z');
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("57014"),
            "unexpected pgwire error fields: {error_fields:?}",
        );
        assert!(
            error_fields.iter().any(|(field, value)| {
                *field == 'M' && value.contains("query timeout exceeded")
            }),
            "deadline error should mention query timeout"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
