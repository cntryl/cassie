//! Two independent wire connections around a first-snapshot boundary.

use std::time::Duration;

use crate::support_pgwire as wire;

pub type Frames = Vec<(u8, Vec<u8>)>;

pub fn run_boundary(
    before_begin: Vec<Vec<u8>>,
    boundary: Vec<Vec<u8>>,
    readback: Vec<Vec<u8>>,
) -> (Frames, Frames) {
    let (mut retained, fresh) = run_script(vec![before_begin], boundary, vec![readback]);
    (retained.pop().expect("one readback"), fresh)
}

pub fn run_script(
    before_begin: Vec<Vec<Vec<u8>>>,
    boundary: Vec<Vec<u8>>,
    readbacks: Vec<Vec<Vec<u8>>>,
) -> (Vec<Frames>, Frames) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE rr_boundary_rows (id INT PRIMARY KEY)",
            "INSERT INTO rr_boundary_rows VALUES (1)",
        ] {
            cassie.execute_sql(&session, sql, Vec::new()).expect("seed");
        }
        let server = wire::spawn_server(cassie).await;
        let reader = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("reader");
        let writer = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("writer");
        let (mut reader_read, mut reader_write) = reader.into_split();
        let (mut writer_read, mut writer_write) = writer.into_split();
        wire::complete_startup(&mut reader_read, &mut reader_write).await;
        wire::complete_startup(&mut writer_read, &mut writer_write).await;
        for frames in before_begin.into_iter().filter(|frames| !frames.is_empty()) {
            exchange(&mut reader_read, &mut reader_write, frames).await;
        }
        exchange(
            &mut reader_read,
            &mut reader_write,
            vec![wire::simple_query_frame(
                "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            )],
        )
        .await;
        exchange(&mut reader_read, &mut reader_write, boundary).await;
        exchange(
            &mut writer_read,
            &mut writer_write,
            vec![wire::simple_query_frame(
                "INSERT INTO rr_boundary_rows VALUES (3)",
            )],
        )
        .await;
        let mut retained = Vec::new();
        for frames in readbacks {
            retained.push(exchange(&mut reader_read, &mut reader_write, frames).await);
        }
        exchange(
            &mut reader_read,
            &mut reader_write,
            vec![wire::simple_query_frame("COMMIT")],
        )
        .await;
        let fresh = exchange(
            &mut reader_read,
            &mut reader_write,
            vec![wire::simple_query_frame(
                "SELECT count(*) FROM rr_boundary_rows",
            )],
        )
        .await;
        drop((reader_read, reader_write, writer_read, writer_write));
        server.stop().await;
        (retained, fresh)
    })
}

async fn exchange(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    frames: Vec<Vec<u8>>,
) -> Frames {
    wire::write_frames(writer, frames).await;
    let frames = wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await;
    assert_eq!(wire::error_code(&frames), None, "{frames:?}");
    frames
}

pub fn assert_counts((retained, fresh): &(Frames, Frames), expected: &str) {
    assert_eq!(
        wire::data_rows(retained),
        vec![vec![Some(expected.to_owned())]]
    );
    assert_eq!(wire::data_rows(fresh), vec![vec![Some("2".to_owned())]]);
}

pub fn disconnect_retained_view() -> (u64, u64) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let observer = cassie.clone();
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("reader");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup(&mut reader, &mut writer).await;
        exchange(
            &mut reader,
            &mut writer,
            vec![wire::simple_query_frame(
                "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY; SELECT 1",
            )],
        )
        .await;
        let retained = observer.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("retained charge");

        wire::write_frames(&mut writer, vec![vec![b'X', 0, 0, 0, 4]]).await;
        let mut byte = [0];
        let eof = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::AsyncReadExt::read(&mut reader, &mut byte),
        )
        .await
        .expect("Terminate closes connection after state cleanup")
        .expect("EOF");
        assert_eq!(eof, 0);
        let released = observer.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("released charge");
        drop((reader, writer));
        server.stop().await;
        (retained, released)
    })
}

/// Executes ordered protocol exchanges on one owned memory-mode connection.
pub fn run_own_script(setup: &[&str], script: Vec<Vec<Vec<u8>>>) -> Vec<Frames> {
    run_own_script_with_expected_errors(setup, script, &[])
}

/// Executes one connection's script with exact expected SQLSTATEs at selected
/// zero-based exchange indexes. Every other exchange must have no error frame.
pub fn run_own_script_with_expected_errors(
    setup: &[&str],
    script: Vec<Vec<Vec<u8>>>,
    expected_errors: &[(usize, &str)],
) -> Vec<Frames> {
    for (position, (index, _)) in expected_errors.iter().enumerate() {
        assert!(
            *index < script.len(),
            "expected error index is outside the script"
        );
        assert!(
            !expected_errors[..position]
                .iter()
                .any(|(earlier, _)| earlier == index),
            "duplicate expected error index {index}"
        );
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in setup {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("setup");
        }
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("client");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup(&mut reader, &mut writer).await;
        let mut responses = Vec::new();
        for (index, frames) in script.into_iter().enumerate() {
            wire::write_frames(&mut writer, frames).await;
            let response =
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
            let expected = expected_errors
                .iter()
                .find_map(|(step, code)| (*step == index).then_some(*code));
            assert_script_response(&response, index, expected);
            responses.push(response);
        }
        drop((reader, writer));
        server.stop().await;
        responses
    })
}

fn assert_script_response(frames: &Frames, index: usize, expected: Option<&str>) {
    let count = frames.iter().filter(|(tag, _)| *tag == b'E').count();
    assert_eq!(
        count,
        usize::from(expected.is_some()),
        "script step {index}: {frames:?}"
    );
    let actual = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, payload)| complete_sqlstate(payload));
    assert_eq!(actual, expected, "script step {index}: {frames:?}");
}

fn complete_sqlstate(payload: &[u8]) -> Option<&str> {
    let mut index = 0;
    let mut state = None;
    loop {
        let tag = *payload.get(index)?;
        index += 1;
        if tag == 0 {
            return (index == payload.len()).then_some(state?);
        }
        let end = index.checked_add(payload.get(index..)?.iter().position(|byte| *byte == 0)?)?;
        if tag == b'C' {
            if state.is_some() {
                return None;
            }
            let value = std::str::from_utf8(&payload[index..end]).ok()?;
            if value.len() != 5 {
                return None;
            }
            state = Some(value);
        }
        index = end + 1;
    }
}

#[test]
fn should_require_exact_error_count_and_sqlstate_for_each_script_step() {
    // Arrange
    let success = vec![(b'Z', b"I".to_vec())];
    let failure = vec![(b'E', b"SERROR\0C22012\0Mdivision by zero\0\0".to_vec())];

    // Act / Assert
    assert_script_response(&success, 0, None);
    assert_script_response(&failure, 1, Some("22012"));
    assert!(std::panic::catch_unwind(|| assert_script_response(&failure, 1, None)).is_err());
    assert!(
        std::panic::catch_unwind(|| assert_script_response(&failure, 1, Some("42601"))).is_err()
    );
    assert!(
        std::panic::catch_unwind(|| assert_script_response(&success, 1, Some("22012"))).is_err()
    );
    let duplicate = vec![failure[0].clone(), failure[0].clone()];
    assert!(
        std::panic::catch_unwind(|| assert_script_response(&duplicate, 1, Some("22012"))).is_err()
    );
    let duplicate_code = vec![(b'E', b"C22012\0C42601\0\0".to_vec())];
    let unterminated = vec![(b'E', b"C22012\0".to_vec())];
    assert!(std::panic::catch_unwind(|| {
        assert_script_response(&duplicate_code, 1, Some("22012"));
    })
    .is_err());
    assert!(std::panic::catch_unwind(|| {
        assert_script_response(&unterminated, 1, Some("22012"));
    })
    .is_err());
}
