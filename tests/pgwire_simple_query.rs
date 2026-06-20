use std::time::Duration;

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
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "simple_query_docs";
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
        let (auth_tag, auth_payload) = read_wire_frame(&mut reader).await;
        assert_eq!(auth_tag, b'R', "startup should return an auth response");
        assert_eq!(
            i32::from_be_bytes(auth_payload[0..4].try_into().expect("auth status")),
            0,
            "passwordless auth should succeed"
        );
        let startup_ready = read_wire_frame(&mut reader).await;
        assert_eq!(startup_ready.0, b'Z', "startup should end ready-for-query");
        assert_eq!(startup_ready.1, vec![b'I']);

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT title FROM simple_query_docs ORDER BY title"),
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
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "simple_query_empty_docs";
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
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        tokio::io::AsyncWriteExt::write_all(&mut write_half, &startup_frame("postgres", "testdb"))
            .await
            .expect("write startup");
        let auth = read_wire_frame(&mut reader).await;
        assert_eq!(auth.0, b'R', "startup should return an auth response");
        let startup_ready = read_wire_frame(&mut reader).await;
        assert_eq!(startup_ready.0, b'Z', "startup should end ready-for-query");

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
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

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
        let startup_ready = read_wire_frame(&mut reader).await;
        assert_eq!(
            startup_ready.0, b'Z',
            "startup should end ready-for-query"
        );
        assert_eq!(startup_ready.1, vec![b'I']);

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
        assert_eq!(ready.0, b'Z', "query failure should still return ready-for-query");
        assert!(
            parse_error_fields(&error.1)
                .iter()
                .any(|(field, value)| *field == 'M' && value.contains("missing_simple_query_table")),
            "error response should mention the missing table"
        );
        assert_eq!(ready.1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
