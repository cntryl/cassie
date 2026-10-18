use std::time::Duration;

use cassie::app::Cassie;
use cassie::types::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::tcp::{ReadHalf, WriteHalf};

use super::support_pgwire as support;

const ANSWER_LIMIT: Duration = Duration::from_secs(10);
const COPY_ROWS_SQL: &str =
    "COPY pgwire_copy_protocol_rows (id, title) FROM STDIN WITH (FORMAT csv)";

type Reader<'a> = tokio::io::BufReader<ReadHalf<'a>>;

fn configured_cassie(label: &str) -> (Cassie, String) {
    support::use_local_storage();
    let path = support::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE pgwire_copy_protocol_rows (id INT, title TEXT)",
            vec![],
        )
        .expect("create copy table");
    (cassie, path)
}

fn row_count(cassie: &Cassie) -> Value {
    let session = cassie.create_session("tester", None);
    let result = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM pgwire_copy_protocol_rows",
            vec![],
        )
        .expect("count copied rows");
    result.rows()[0][0].clone()
}

async fn query(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    sql: &str,
) -> Vec<(u8, Vec<u8>)> {
    support::write_frames(writer, vec![support::simple_query_frame(sql)]).await;
    support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await
}

async fn start_copy(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    sql: &str,
) {
    support::write_frames(writer, vec![support::simple_query_frame(sql)]).await;
    assert_eq!(support::read_wire_frame(reader).await.0, b'G');
}

fn ready_status(frames: &[(u8, Vec<u8>)]) -> u8 {
    let (tag, payload) = frames.last().expect("ReadyForQuery frame");
    assert_eq!(*tag, b'Z');
    payload[0]
}

fn select_one_rows() -> Vec<Vec<Option<String>>> {
    vec![vec![Some("1".to_string())]]
}

/// Serves `cassie` over pgwire and runs `scenario` on one connection. The
/// caller removes the data directory after any post-connection assertions.
fn run_as<F>(cassie: Cassie, user: (&str, &str), scenario: F)
where
    F: for<'a, 'b> AsyncFnOnce(&'b mut Reader<'a>, &'b mut WriteHalf<'a>),
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        support::complete_startup_as(&mut reader, &mut writer, user.0, "postgres", user.1).await;
        scenario(&mut reader, &mut writer).await;
        server.stop().await;
    });
}

fn run<F>(cassie: Cassie, scenario: F)
where
    F: for<'a, 'b> AsyncFnOnce(&'b mut Reader<'a>, &'b mut WriteHalf<'a>),
{
    run_as(cassie, ("root", "postgres"), scenario);
}

#[test]
fn should_frame_copy_out_response_as_one_text_column_for_database_backup() {
    // Arrange
    let (cassie, path) = configured_cassie("copy-out-framing");

    run(cassie, async |reader, writer| {
        // Act
        let backup = query(reader, writer, "BACKUP DATABASE postgres TO STDOUT").await;

        // Assert
        let (tag, body) = &backup[0];
        assert_eq!(*tag, b'H');
        let columns = usize::try_from(i16::from_be_bytes([body[1], body[2]])).expect("count");
        assert_eq!(body[0], 0);
        assert_eq!(columns, 1);
        assert_eq!(body.len(), 3 + 2 * columns);
        assert_eq!(body.as_slice(), &[0, 0, 1, 0, 0]);
        assert!(backup.iter().any(|frame| frame.0 == b'c'));
        assert_eq!(ready_status(&backup), b'I');
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_abort_open_transaction_given_copy_bind_error() {
    // Arrange
    let (cassie, path) = configured_cassie("copy-bind-error-transaction");
    let observer = cassie.clone();

    run(cassie, async |reader, writer| {
        let begin = query(reader, writer, "BEGIN").await;
        assert_eq!(ready_status(&begin), b'T');

        // Act
        let copy = query(
            reader,
            writer,
            "COPY pgwire_copy_protocol_missing FROM STDIN WITH (FORMAT csv)",
        )
        .await;
        let insert = query(
            reader,
            writer,
            "INSERT INTO pgwire_copy_protocol_rows (id, title) VALUES (1, 'kept')",
        )
        .await;
        let _ = query(reader, writer, "COMMIT").await;

        // Assert
        assert!(copy.iter().all(|frame| frame.0 != b'G'));
        assert!(support::error_code(&copy).is_some());
        assert_eq!(ready_status(&copy), b'E');
        assert!(support::error_code(&insert).is_some());
        assert_eq!(ready_status(&insert), b'E');
    });
    assert_eq!(row_count(&observer), Value::Int64(0));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_abort_open_transaction_given_database_copy_privilege_denial() {
    // Arrange
    let (cassie, path) = configured_cassie("database-copy-denial-transaction");
    let admin = cassie
        .authenticate_role("root", Some("postgres"), None)
        .expect("admin");
    for sql in [
        "CREATE ROLE copy_reader LOGIN PASSWORD 'reader-secret'",
        "CREATE DATABASE copy_denied",
        "GRANT CONNECT ON DATABASE postgres TO copy_reader",
    ] {
        cassie
            .execute_sql(&admin, sql, vec![])
            .expect("role fixture");
    }

    run_as(
        cassie,
        ("copy_reader", "reader-secret"),
        async |reader, writer| {
            let begin = query(reader, writer, "BEGIN").await;
            assert_eq!(ready_status(&begin), b'T');

            // Act
            let backup = query(reader, writer, "BACKUP DATABASE copy_denied TO STDOUT").await;

            // Assert
            assert_eq!(support::error_code(&backup).as_deref(), Some("42501"));
            assert_eq!(ready_status(&backup), b'E');
        },
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_answer_next_query_after_copy_payload_exceeds_resource_limit() {
    // Arrange
    let (cassie, path) = configured_cassie("copy-resource-limit-drain");
    let chunk = vec![b'x'; 64 * 1024];

    run(cassie, async |reader, writer| {
        start_copy(reader, writer, COPY_ROWS_SQL).await;
        let mut frames = (0..260)
            .map(|_| support::copy_data_frame(&chunk))
            .collect::<Vec<_>>();
        frames.push(support::copy_done_frame());
        frames.push(support::simple_query_frame("SELECT 1"));

        // Act
        support::write_frames(writer, frames).await;
        let failed = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;
        let recovered = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

        // Assert
        assert_eq!(support::error_code(&failed).as_deref(), Some("54000"));
        assert_eq!(ready_status(&failed), b'I');
        assert_eq!(support::error_code(&recovered), None);
        assert_eq!(support::data_rows(&recovered), select_one_rows());
        assert_eq!(ready_status(&recovered), b'I');
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_accept_binary_copy_from_with_int4_null_framing() {
    // Arrange
    let (cassie, path) = configured_cassie("binary-copy-from-int4-null");
    let mut binary_payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    binary_payload.extend_from_slice(&0_i32.to_be_bytes());
    binary_payload.extend_from_slice(&0_i32.to_be_bytes());
    for (id, title) in [(1_i32, Some(&b"alpha"[..])), (2_i32, None)] {
        binary_payload.extend_from_slice(&2_i16.to_be_bytes());
        binary_payload.extend_from_slice(&4_i32.to_be_bytes());
        binary_payload.extend_from_slice(&id.to_be_bytes());
        if let Some(title) = title {
            binary_payload.extend_from_slice(
                &i32::try_from(title.len())
                    .expect("binary text length")
                    .to_be_bytes(),
            );
            binary_payload.extend_from_slice(title);
        } else {
            binary_payload.extend_from_slice(&(-1_i32).to_be_bytes());
        }
    }
    binary_payload.extend_from_slice(&(-1_i16).to_be_bytes());

    let observed_copy_response = std::sync::Arc::new(std::sync::Mutex::new(None::<(u8, Vec<u8>)>));
    let observed_copy_result =
        std::sync::Arc::new(std::sync::Mutex::new(None::<Vec<(u8, Vec<u8>)>>));
    let observed_rows =
        std::sync::Arc::new(std::sync::Mutex::new(None::<Vec<Vec<Option<String>>>>));
    let copy_response_result = std::sync::Arc::clone(&observed_copy_response);
    let copy_result = std::sync::Arc::clone(&observed_copy_result);
    let rows_result = std::sync::Arc::clone(&observed_rows);

    run(cassie, async |reader, writer| {
        support::write_frames(
            writer,
            vec![support::simple_query_frame(
                "COPY pgwire_copy_protocol_rows (id, title) FROM STDIN WITH (FORMAT BINARY)",
            )],
        )
        .await;
        let (response_tag, response_payload) = support::read_wire_frame(reader).await;
        *copy_response_result.lock().expect("COPY response lock") =
            Some((response_tag, response_payload.clone()));
        if response_tag != b'G' || response_payload.as_slice() != [1_u8, 0, 2, 0, 1, 0, 1] {
            return;
        }

        // Act
        support::write_frames(
            writer,
            vec![
                support::copy_data_frame(&binary_payload),
                support::copy_done_frame(),
            ],
        )
        .await;
        let copied = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;
        let rows = query(
            reader,
            writer,
            "SELECT id, title FROM pgwire_copy_protocol_rows ORDER BY id",
        )
        .await;
        *copy_result.lock().expect("COPY result lock") = Some(copied);
        *rows_result.lock().expect("COPY rows lock") = Some(support::data_rows(&rows));
    });
    let _ = std::fs::remove_dir_all(path);

    // Assert
    let (response_tag, response_payload) = observed_copy_response
        .lock()
        .expect("COPY response lock")
        .take()
        .expect("server should respond to COPY FROM");
    assert_eq!(response_tag, b'G', "binary COPY should enter CopyIn mode");
    assert_eq!(response_payload, vec![1, 0, 2, 0, 1, 0, 1]);
    let copied = observed_copy_result
        .lock()
        .expect("COPY result lock")
        .take()
        .expect("binary COPY payload should complete");
    assert_eq!(support::error_code(&copied), None);
    assert_eq!(ready_status(&copied), b'I');
    let rows = observed_rows
        .lock()
        .expect("COPY rows lock")
        .take()
        .expect("query should return copied rows");
    assert_eq!(
        rows,
        vec![
            vec![Some("1".to_string()), Some("alpha".to_string())],
            vec![Some("2".to_string()), None],
        ]
    );
}

#[test]
fn should_ignore_synchronization_messages_during_copy_in() {
    // Arrange
    let (cassie, path) = configured_cassie("copy-flush-sync");
    let observer = cassie.clone();

    run(cassie, async |reader, writer| {
        start_copy(reader, writer, COPY_ROWS_SQL).await;
        let frames = vec![
            support::copy_data_frame(b"1,alpha\n"),
            support::flush_frame(),
            support::copy_data_frame(b"2,beta\n"),
            support::sync_frame(),
            support::copy_done_frame(),
        ];

        // Act
        support::write_frames(writer, frames).await;
        let copied = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

        // Assert
        assert_eq!(support::error_code(&copied), None);
        let complete = copied.iter().find(|frame| frame.0 == b'C');
        assert_eq!(
            complete.map(|frame| frame.1.as_slice()),
            Some(&b"COPY 2\0"[..])
        );
        assert_eq!(copied.iter().filter(|frame| frame.0 == b'Z').count(), 1);
        assert_eq!(ready_status(&copied), b'I');
    });
    assert_eq!(row_count(&observer), Value::Int64(2));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_answer_next_query_after_copy_fail_with_trailing_copy_messages() {
    // Arrange
    let (cassie, path) = configured_cassie("copy-fail-trailing");
    let observer = cassie.clone();

    run(cassie, async |reader, writer| {
        start_copy(reader, writer, COPY_ROWS_SQL).await;
        let frames = vec![
            support::copy_data_frame(b"1,alpha\n"),
            support::copy_fail_frame("client aborted"),
            support::copy_data_frame(b"2,late\n"),
            support::copy_done_frame(),
            support::simple_query_frame("SELECT 1"),
        ];

        // Act
        support::write_frames(writer, frames).await;
        let failed = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;
        let recovered = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

        // Assert
        assert_eq!(support::error_code(&failed).as_deref(), Some("57014"));
        assert_eq!(support::error_code(&recovered), None);
        assert_eq!(support::data_rows(&recovered), select_one_rows());
    });
    assert_eq!(row_count(&observer), Value::Int64(0));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_answer_next_query_after_rejected_restore_image() {
    // Arrange
    let (cassie, path) = configured_cassie("restore-rejected-image");

    run(cassie, async |reader, writer| {
        start_copy(
            reader,
            writer,
            "RESTORE DATABASE restore_rejected FROM STDIN",
        )
        .await;
        let frames = vec![
            support::copy_data_frame(b"NOTCASSIE-bad-image-bytes"),
            support::copy_data_frame(b"more-bad-image-bytes"),
            support::copy_done_frame(),
            support::simple_query_frame("SELECT 1"),
        ];

        // Act
        support::write_frames(writer, frames).await;
        let failed = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;
        let recovered = support::read_frames_until_ready_within(reader, ANSWER_LIMIT).await;

        // Assert
        assert!(support::error_code(&failed).is_some());
        assert_eq!(ready_status(&failed), b'I');
        assert_eq!(support::error_code(&recovered), None);
        assert_eq!(support::data_rows(&recovered), select_one_rows());
        assert_eq!(ready_status(&recovered), b'I');
    });
    let _ = std::fs::remove_dir_all(path);
}
