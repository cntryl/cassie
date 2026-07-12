use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use std::net::SocketAddr;
use std::time::Duration;

type PgwireServer = tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>;
type WireFrame = (u8, Vec<u8>);

#[path = "support/sql.rs"]
mod support;
use support::*;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-pgwire-simple-query-batch-{label}"));
    path.push(uuid::Uuid::new_v4().to_string());
    path.to_string_lossy().to_string()
}

fn new_cassie(path: &str) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password.clear();
    Cassie::new_with_data_dir_and_config(path, config).expect("cassie")
}

fn startup_frame() -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&0x0003_0000_i32.to_be_bytes());
    payload.extend_from_slice(b"user\0postgres\0database\0postgres\0\0");

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
    let mut payload = sql.as_bytes().to_vec();
    payload.push(0);

    let mut frame = vec![b'Q'];
    frame.extend_from_slice(
        &i32::try_from(payload.len() + 4)
            .expect("query payload size must fit into i32")
            .to_be_bytes(),
    );
    frame.extend_from_slice(&payload);
    frame
}

async fn spawn_server(cassie: &Cassie) -> (SocketAddr, PgwireServer) {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password.clear();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let address = listener.local_addr().expect("listener address");
    drop(listener);
    let server = tokio::spawn(cassie::pgwire::server::run(
        address.to_string(),
        std::sync::Arc::new(cassie.clone()),
        config,
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    (address, server)
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> WireFrame {
    let mut tag = [0_u8; 1];
    tokio::io::AsyncReadExt::read_exact(reader, &mut tag)
        .await
        .expect("read frame tag");
    let mut length = [0_u8; 4];
    tokio::io::AsyncReadExt::read_exact(reader, &mut length)
        .await
        .expect("read frame length");
    let length = i32::from_be_bytes(length);
    let payload_length = usize::try_from(length - 4).expect("valid frame length");
    let mut payload = vec![0_u8; payload_length];
    if payload_length > 0 {
        tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
            .await
            .expect("read frame payload");
    }
    (tag[0], payload)
}

async fn start_session(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
) {
    tokio::io::AsyncWriteExt::write_all(writer, &startup_frame())
        .await
        .expect("write startup");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush startup");
    let authentication = read_wire_frame(reader).await;
    assert_eq!(authentication.0, b'R');
    assert_eq!(
        i32::from_be_bytes(
            authentication.1[0..4]
                .try_into()
                .expect("authentication status"),
        ),
        0
    );
    loop {
        let frame = read_wire_frame(reader).await;
        if frame.0 == b'Z' {
            assert_eq!(frame.1, vec![b'I']);
            break;
        }
        assert_eq!(
            frame.0, b'S',
            "startup should only emit parameter statuses before ready"
        );
    }
}

async fn send_query(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
    sql: &str,
) -> Vec<WireFrame> {
    tokio::io::AsyncWriteExt::write_all(writer, &simple_query_frame(sql))
        .await
        .expect("write query");
    tokio::io::AsyncWriteExt::flush(writer)
        .await
        .expect("flush query");
    let mut frames = Vec::new();
    loop {
        let frame = read_wire_frame(reader).await;
        let ready = frame.0 == b'Z';
        frames.push(frame);
        if ready {
            return frames;
        }
    }
}

fn cstring(payload: &[u8]) -> String {
    let end = payload
        .iter()
        .position(|byte| *byte == 0)
        .expect("cstring terminator");
    String::from_utf8(payload[..end].to_vec()).expect("cstring utf-8")
}

fn command(frame: &WireFrame) -> String {
    assert_eq!(frame.0, b'C', "expected command complete frame");
    cstring(&frame.1)
}

fn data_row(frame: &WireFrame) -> Vec<Option<String>> {
    assert_eq!(frame.0, b'D', "expected data row frame");
    let mut cursor = 0usize;
    let count = i16::from_be_bytes(frame.1[0..2].try_into().expect("column count"));
    cursor += 2;
    let mut values = Vec::new();
    for _ in 0..count {
        let length = i32::from_be_bytes(
            frame.1[cursor..cursor + 4]
                .try_into()
                .expect("value length"),
        );
        cursor += 4;
        if length < 0 {
            values.push(None);
            continue;
        }
        let length = usize::try_from(length).expect("value length fits usize");
        let end = cursor + length;
        values.push(Some(
            String::from_utf8(frame.1[cursor..end].to_vec()).expect("data row utf-8"),
        ));
        cursor = end;
    }
    values
}

fn error_code(frame: &WireFrame) -> Option<String> {
    assert_eq!(frame.0, b'E', "expected error response frame");
    let mut cursor = 0usize;
    while cursor < frame.1.len() && frame.1[cursor] != 0 {
        let field = frame.1[cursor];
        cursor += 1;
        let remaining = &frame.1[cursor..];
        let end = remaining
            .iter()
            .position(|byte| *byte == 0)
            .expect("error field terminator");
        if field == b'C' {
            return Some(String::from_utf8(remaining[..end].to_vec()).expect("sqlstate utf-8"));
        }
        cursor += end + 1;
    }
    None
}

fn assert_ready(frames: &[WireFrame]) {
    assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(b"I".as_slice())
    );
}

#[test]
fn should_execute_simple_query_statements_in_order_with_one_ready_frame() {
    // Arrange
    with_fallback();
    let path = data_dir("ordered");

    runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_order (number INT); INSERT INTO batch_order (number) VALUES (1); SELECT number FROM batch_order ORDER BY number",
        )
        .await;
        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(command(&frames[0]), "CREATE TABLE");
        assert_eq!(command(&frames[1]), "INSERT 0 1");
        assert_eq!(data_row(&frames[3]), vec![Some("1".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_semicolon_delimiters_in_quoted_commented_sql() {
    // Arrange
    with_fallback();
    let path = data_dir("quotes-comments");

    runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE \"batch;quoted\" (note TEXT); -- comment;\nINSERT INTO \"batch;quoted\" (note) VALUES ('text;value'); /* block; comment */ SELECT note FROM \"batch;quoted\"",
        )
        .await;
        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(data_row(&frames[3]), vec![Some("text;value".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_empty_statements_in_a_simple_query_batch() {
    // Arrange
    with_fallback();
    let path = data_dir("empty");

    runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            ";; CREATE TABLE batch_empty (number INT); ; INSERT INTO batch_empty (number) VALUES (7); ;; SELECT number FROM batch_empty",
        )
        .await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(data_row(&frames[3]), vec![Some("7".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_stop_after_the_first_error_in_a_simple_query_batch() {
    // Arrange
    with_fallback();
    let path = data_dir("stop-on-error");

    runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_stop (number INT); INSERT INTO batch_stop (number) VALUES (1); SELECT number FROM missing_batch_stop; INSERT INTO batch_stop (number) VALUES (2)",
        )
        .await;
        let after_error = send_query(
            &mut reader,
            &mut writer,
            "SELECT number FROM batch_stop ORDER BY number",
        )
        .await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'E', b'Z']
        );
        assert_ready(&frames);
        assert_eq!(
            after_error.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'T', b'D', b'C', b'Z']
        );
        assert_eq!(data_row(&after_error[1]), vec![Some("1".to_string())]);
        assert_ready(&after_error);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_transaction_control_statements_in_order() {
    // Arrange
    with_fallback();
    let path = data_dir("transactions");

    runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_transaction (number INT); BEGIN; INSERT INTO batch_transaction (number) VALUES (9); COMMIT; SELECT number FROM batch_transaction",
        )
        .await;

        // Assert
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'C', b'C', b'C', b'C', b'T', b'D', b'C', b'Z']
        );
        assert_eq!(command(&frames[1]), "BEGIN");
        assert_eq!(command(&frames[2]), "INSERT 0 1");
        assert_eq!(command(&frames[3]), "COMMIT");
        assert_eq!(data_row(&frames[5]), vec![Some("9".to_string())]);
        assert_ready(&frames);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_copy_when_mixed_with_other_simple_query_statements() {
    // Arrange
    with_fallback();
    let path = data_dir("copy-mixed");

    runtime().block_on(async {
        let cassie = new_cassie(&path);
        cassie.startup().expect("startup");
        let (address, server) = spawn_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_session(&mut reader, &mut writer).await;

        // Act
        let frames = send_query(
            &mut reader,
            &mut writer,
            "CREATE TABLE batch_copy (number INT); COPY batch_copy FROM STDIN WITH (FORMAT csv); SELECT number FROM batch_copy",
        )
        .await;
        let no_partial_table = send_query(&mut reader, &mut writer, "SELECT number FROM batch_copy").await;

        // Assert
        assert_eq!(frames.iter().map(|frame| frame.0).collect::<Vec<_>>(), vec![b'E', b'Z']);
        assert_eq!(error_code(&frames[0]).as_deref(), Some("0A000"));
        assert_ready(&frames);
        assert_eq!(
            no_partial_table.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            vec![b'E', b'Z']
        );
        assert_eq!(error_code(&no_partial_table[0]).as_deref(), Some("42P01"));
        assert_ready(&no_partial_table);

        drop(reader);
        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
