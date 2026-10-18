//! PostgreSQL 18.6 advisory-lock and LISTEN/NOTIFY wire semantics for #822.

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

type Frames = Vec<(u8, Vec<u8>)>;

struct FixtureDirectory(String);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}

async fn connect_session(
    server: &wire::PgwireServer,
) -> (
    tokio::net::tcp::OwnedReadHalf,
    tokio::net::tcp::OwnedWriteHalf,
) {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect pgwire session");
    let (mut reader, mut writer) = socket.into_split();
    wire::complete_startup(&mut reader, &mut writer).await;
    (reader, writer)
}

async fn simple_query(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    sql: &str,
) -> Frames {
    wire::write_frames(writer, vec![wire::simple_query_frame(sql)]).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

fn assert_scalar_result(frames: &Frames, expected_oid: i32, expected_value: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "unexpected wire error: {frames:?}"
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("scalar RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].type_oid, expected_oid);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(expected_value.to_owned())]]
    );
    assert_eq!(frames.last().map(|(tag, _)| *tag), Some(b'Z'));
}

fn assert_command(frames: &Frames, expected_tag: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "unexpected wire error: {frames:?}"
    );
    let command = frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            let end = payload
                .iter()
                .position(|byte| *byte == 0)
                .expect("CommandComplete tag terminator");
            std::str::from_utf8(&payload[..end])
                .expect("CommandComplete tag is UTF-8")
                .to_owned()
        })
        .expect("CommandComplete");
    assert_eq!(command, expected_tag);
    assert_eq!(frames.last().map(|(tag, _)| *tag), Some(b'Z'));
}

#[test]
fn should_observe_advisory_lock_ownership_across_pgwire_sessions() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-822-advisory-lock-pgwire-sessions");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let (
        owner_lock,
        contender_busy,
        owner_unlock,
        contender_acquire,
        contender_unlock,
        owner_recovery,
        contender_recovery,
    ) = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("start advisory-lock fixture");
        let server = wire::spawn_server(cassie).await;
        let (mut owner_reader, mut owner_writer) = connect_session(&server).await;
        let (mut contender_reader, mut contender_writer) = connect_session(&server).await;

        let owner_lock = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_lock(82201822, 61)",
        )
        .await;
        let contender_busy = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201822, 61)",
        )
        .await;
        let owner_unlock = simple_query(
            &mut owner_reader,
            &mut owner_writer,
            "SELECT pg_advisory_unlock(82201822, 61)",
        )
        .await;
        let contender_acquire = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_try_advisory_lock(82201822, 61)",
        )
        .await;
        let contender_unlock = simple_query(
            &mut contender_reader,
            &mut contender_writer,
            "SELECT pg_advisory_unlock(82201822, 61)",
        )
        .await;
        let owner_recovery = simple_query(&mut owner_reader, &mut owner_writer, "SELECT 44").await;
        let contender_recovery =
            simple_query(&mut contender_reader, &mut contender_writer, "SELECT 45").await;

        drop((
            owner_reader,
            owner_writer,
            contender_reader,
            contender_writer,
        ));
        server.stop().await;
        (
            owner_lock,
            contender_busy,
            owner_unlock,
            contender_acquire,
            contender_unlock,
            owner_recovery,
            contender_recovery,
        )
    });
    drop(runtime);

    // Assert
    assert_scalar_result(&owner_lock, 2278, "");
    assert_scalar_result(&contender_busy, 16, "f");
    assert_scalar_result(&owner_unlock, 16, "t");
    assert_scalar_result(&contender_acquire, 16, "t");
    assert_scalar_result(&contender_unlock, 16, "t");
    assert_scalar_result(&owner_recovery, 23, "44");
    assert_scalar_result(&contender_recovery, 23, "45");
}

struct UnlockAllResults {
    owner_begin: Frames,
    owner_one_key_lock: Frames,
    owner_two_key_lock: Frames,
    owner_xact_lock: Frames,
    contender_one_key_busy: Frames,
    contender_two_key_busy: Frames,
    contender_xact_busy: Frames,
    owner_unlock_all: Frames,
    contender_one_key_acquire: Frames,
    contender_two_key_acquire: Frames,
    contender_one_key_unlock: Frames,
    contender_two_key_unlock: Frames,
    contender_xact_still_busy: Frames,
    owner_rollback: Frames,
    contender_xact_acquire: Frames,
    recovery: Frames,
}

async fn acquire_owner_advisory_locks(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
) -> (Frames, Frames, Frames, Frames) {
    let owner_begin = simple_query(reader, writer, "BEGIN").await;
    let owner_one_key_lock =
        simple_query(reader, writer, "SELECT pg_advisory_lock(82201823)").await;
    let owner_two_key_lock =
        simple_query(reader, writer, "SELECT pg_advisory_lock(82201822, 62)").await;
    let owner_xact_lock =
        simple_query(reader, writer, "SELECT pg_advisory_xact_lock(82201824, 63)").await;
    (
        owner_begin,
        owner_one_key_lock,
        owner_two_key_lock,
        owner_xact_lock,
    )
}

async fn run_unlock_all_queries(
    owner_reader: &mut (impl AsyncRead + Unpin),
    owner_writer: &mut (impl AsyncWrite + Unpin),
    contender_reader: &mut (impl AsyncRead + Unpin),
    contender_writer: &mut (impl AsyncWrite + Unpin),
) -> UnlockAllResults {
    let (owner_begin, owner_one_key_lock, owner_two_key_lock, owner_xact_lock) =
        acquire_owner_advisory_locks(owner_reader, owner_writer).await;
    let contender_one_key_busy = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_try_advisory_lock(82201823)",
    )
    .await;
    let contender_two_key_busy = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_try_advisory_lock(82201822, 62)",
    )
    .await;
    let contender_xact_busy = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_try_advisory_xact_lock(82201824, 63)",
    )
    .await;
    let owner_unlock_all = simple_query(
        owner_reader,
        owner_writer,
        "SELECT pg_advisory_unlock_all()",
    )
    .await;
    let contender_one_key_acquire = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_try_advisory_lock(82201823)",
    )
    .await;
    let contender_two_key_acquire = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_try_advisory_lock(82201822, 62)",
    )
    .await;
    let contender_one_key_unlock = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_advisory_unlock(82201823)",
    )
    .await;
    let contender_two_key_unlock = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_advisory_unlock(82201822, 62)",
    )
    .await;
    let contender_xact_still_busy = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_try_advisory_xact_lock(82201824, 63)",
    )
    .await;
    let owner_rollback = simple_query(owner_reader, owner_writer, "ROLLBACK").await;
    let contender_xact_acquire = simple_query(
        contender_reader,
        contender_writer,
        "SELECT pg_try_advisory_xact_lock(82201824, 63)",
    )
    .await;
    let recovery = simple_query(contender_reader, contender_writer, "SELECT 47").await;

    UnlockAllResults {
        owner_begin,
        owner_one_key_lock,
        owner_two_key_lock,
        owner_xact_lock,
        contender_one_key_busy,
        contender_two_key_busy,
        contender_xact_busy,
        owner_unlock_all,
        contender_one_key_acquire,
        contender_two_key_acquire,
        contender_one_key_unlock,
        contender_two_key_unlock,
        contender_xact_still_busy,
        owner_rollback,
        contender_xact_acquire,
        recovery,
    }
}

#[test]
fn should_release_session_locks_with_unlock_all_over_pgwire() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("issue-822-advisory-unlock-all-pgwire");
    let _directory = FixtureDirectory(path.clone());
    let runtime = current_thread_runtime();

    // Act
    let observations = runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("start advisory unlock-all fixture");
        let server = wire::spawn_server(cassie).await;
        let (mut owner_reader, mut owner_writer) = connect_session(&server).await;
        let (mut contender_reader, mut contender_writer) = connect_session(&server).await;
        let results = run_unlock_all_queries(
            &mut owner_reader,
            &mut owner_writer,
            &mut contender_reader,
            &mut contender_writer,
        )
        .await;
        drop((
            owner_reader,
            owner_writer,
            contender_reader,
            contender_writer,
        ));
        server.stop().await;
        results
    });
    let UnlockAllResults {
        owner_begin,
        owner_one_key_lock,
        owner_two_key_lock,
        owner_xact_lock,
        contender_one_key_busy,
        contender_two_key_busy,
        contender_xact_busy,
        owner_unlock_all,
        contender_one_key_acquire,
        contender_two_key_acquire,
        contender_one_key_unlock,
        contender_two_key_unlock,
        contender_xact_still_busy,
        owner_rollback,
        contender_xact_acquire,
        recovery,
    } = observations;
    drop(runtime);

    // Assert
    assert_command(&owner_begin, "BEGIN");
    assert_scalar_result(&owner_one_key_lock, 2278, "");
    assert_scalar_result(&owner_two_key_lock, 2278, "");
    assert_scalar_result(&owner_xact_lock, 2278, "");
    assert_scalar_result(&contender_one_key_busy, 16, "f");
    assert_scalar_result(&contender_two_key_busy, 16, "f");
    assert_scalar_result(&contender_xact_busy, 16, "f");
    assert_scalar_result(&owner_unlock_all, 2278, "");
    assert_scalar_result(&contender_one_key_acquire, 16, "t");
    assert_scalar_result(&contender_two_key_acquire, 16, "t");
    assert_scalar_result(&contender_one_key_unlock, 16, "t");
    assert_scalar_result(&contender_two_key_unlock, 16, "t");
    assert_scalar_result(&contender_xact_still_busy, 16, "f");
    assert_command(&owner_rollback, "ROLLBACK");
    assert_scalar_result(&contender_xact_acquire, 16, "t");
    assert_scalar_result(&recovery, 23, "47");
}
