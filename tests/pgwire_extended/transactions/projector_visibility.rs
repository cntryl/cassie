//! Visibility of an order and its application-owned SQL projector checkpoint.

use crate::support_pgwire as wire;
use crate::support_pgwire_cancel_session::{command_tag, ready_status, Frames};
use crate::support_sql;
use crate::support_temp_dirs::IsolatedDataDir;
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const PAIR: &str = "SELECT o.status, c.last_event FROM orders o \
    JOIN projector_checkpoint c ON o.id = c.order_id WHERE o.id = 1";
const SHIP_ORDER: &str = "UPDATE orders SET status = 'shipped' WHERE id = 1";
const ADVANCE: &str = "UPDATE projector_checkpoint SET last_event = 11 WHERE order_id = 1";
const DEADLINE: Duration = Duration::from_secs(5);

fn run<F: std::future::Future>(work: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
        .block_on(work)
}

async fn connect(server: &wire::PgwireServer) -> TcpStream {
    let mut socket = TcpStream::connect(server.addr).await.expect("connect");
    let (mut reader, mut writer) = socket.split();
    wire::complete_startup(&mut reader, &mut writer).await;
    socket
}

async fn query(socket: &mut TcpStream, sql: &str) -> Frames {
    wire::write_frames(socket, vec![wire::simple_query_frame(sql)]).await;
    wire::read_frames_until_ready_within(socket, DEADLINE).await
}

async fn command(socket: &mut TcpStream, sql: &str, tag: &str, state: u8) {
    let frames = query(socket, sql).await;
    assert_eq!(wire::error_code(&frames), None, "{sql}: {frames:?}");
    assert_eq!(command_tag(&frames).as_deref(), Some(tag), "{sql}");
    assert_eq!(ready_status(&frames), Some(state), "{sql}");
}

async fn pair(socket: &mut TcpStream, state: u8) -> Vec<Vec<Option<String>>> {
    let frames = query(socket, PAIR).await;
    assert_eq!(wire::error_code(&frames), None, "{PAIR}: {frames:?}");
    assert_eq!(ready_status(&frames), Some(state));
    wire::data_rows(&frames)
}

fn expected(status: &str, event: &str) -> Vec<Vec<Option<String>>> {
    vec![vec![Some(status.to_owned()), Some(event.to_owned())]]
}

async fn start(cassie: Cassie) -> (wire::PgwireServer, TcpStream, TcpStream) {
    cassie.startup().expect("start Cassie");
    let server = wire::spawn_server(cassie).await;
    let mut writer = connect(&server).await;
    let reader = connect(&server).await;
    for (sql, tag) in [
        (
            "CREATE TABLE orders (id INT PRIMARY KEY, status TEXT NOT NULL)",
            "CREATE TABLE",
        ),
        (
            "CREATE TABLE projector_checkpoint (order_id INT PRIMARY KEY, last_event INT NOT NULL)",
            "CREATE TABLE",
        ),
        ("INSERT INTO orders VALUES (1, 'pending')", "INSERT 0 1"),
        (
            "INSERT INTO projector_checkpoint VALUES (1, 10)",
            "INSERT 0 1",
        ),
    ] {
        command(&mut writer, sql, tag, b'I').await;
    }
    (server, writer, reader)
}

async fn stage_pair(writer: &mut TcpStream) {
    command(writer, "BEGIN", "BEGIN", b'T').await;
    command(writer, SHIP_ORDER, "UPDATE 1", b'T').await;
    command(writer, ADVANCE, "UPDATE 1", b'T').await;
    assert_eq!(pair(writer, b'T').await, expected("shipped", "11"));
}

async fn close_connection(mut socket: TcpStream) {
    // A half-close sends EOF without a Terminate message. Wait until the backend
    // closes its end before using the other connection to inspect the result.
    socket.shutdown().await.expect("send EOF");
    let mut byte = [0];
    let count = tokio::time::timeout(DEADLINE, socket.read(&mut byte))
        .await
        .expect("backend close deadline")
        .expect("backend close");
    assert_eq!(count, 0);
}

#[test]
fn should_publish_the_projector_pair_at_the_reader_isolation_boundary() {
    // Arrange
    for (begin, after_commit) in [
        (
            "BEGIN ISOLATION LEVEL READ COMMITTED",
            expected("shipped", "11"),
        ),
        (
            "BEGIN ISOLATION LEVEL REPEATABLE READ",
            expected("pending", "10"),
        ),
    ] {
        run(async {
            let (server, mut writer, mut reader) =
                start(support_sql::new_in_memory_cassie().expect("memory fixture")).await;
            command(&mut reader, begin, "BEGIN", b'T').await;
            command(&mut writer, "BEGIN", "BEGIN", b'T').await;

            // Act
            command(&mut writer, SHIP_ORDER, "UPDATE 1", b'T').await;
            let half_staged = pair(&mut reader, b'T').await;
            command(&mut writer, ADVANCE, "UPDATE 1", b'T').await;
            let own = pair(&mut writer, b'T').await;
            let before = pair(&mut reader, b'T').await;
            command(&mut writer, "COMMIT", "COMMIT", b'I').await;
            let after = pair(&mut reader, b'T').await;
            command(&mut reader, "COMMIT", "COMMIT", b'I').await;
            let fresh = pair(&mut reader, b'I').await;

            // Assert
            assert_eq!(half_staged, expected("pending", "10"), "{begin}");
            assert_eq!(own, expected("shipped", "11"), "{begin}");
            assert_eq!(before, expected("pending", "10"), "{begin}");
            assert_eq!(after, after_commit, "{begin}");
            assert_eq!(fresh, expected("shipped", "11"), "{begin}");
            server.stop().await;
        });
    }
}

#[test]
fn should_discard_the_projector_pair_on_transaction_abort() {
    // Arrange
    run(async {
        let (server, mut writer, mut reader) =
            start(support_sql::new_in_memory_cassie().expect("memory fixture")).await;

        // Act
        stage_pair(&mut writer).await;
        command(&mut writer, "ROLLBACK", "ROLLBACK", b'I').await;
        let after_rollback = pair(&mut reader, b'I').await;
        stage_pair(&mut writer).await;
        close_connection(writer).await;

        // Assert
        assert_eq!(after_rollback, expected("pending", "10"));
        assert_eq!(pair(&mut reader, b'I').await, expected("pending", "10"));
        server.stop().await;
    });
}

#[test]
fn should_reopen_the_pair_committed_after_savepoint_recovery() {
    // Arrange
    let dir = IsolatedDataDir::new("projector-savepoint");
    let open = || {
        let config =
            support_sql::with_test_root_password(CassieRuntimeConfig::from_env().expect("config"));
        Cassie::new_local_with_data_dir_and_config(dir.path(), config).expect("local fixture")
    };
    run(async {
        let (server, mut writer, mut reader) = start(open()).await;
        command(&mut writer, "BEGIN", "BEGIN", b'T').await;
        command(&mut writer, SHIP_ORDER, "UPDATE 1", b'T').await;
        command(&mut writer, "SAVEPOINT shipped", "SAVEPOINT", b'T').await;

        // Act
        command(
            &mut writer,
            "UPDATE projector_checkpoint SET last_event = 99 WHERE order_id = 1",
            "UPDATE 1",
            b'T',
        )
        .await;
        let provisional = pair(&mut writer, b'T').await;
        let duplicate = query(&mut writer, "INSERT INTO orders VALUES (1, 'duplicate')").await;
        let blocked = query(&mut writer, PAIR).await;
        command(
            &mut writer,
            "ROLLBACK TO SAVEPOINT shipped",
            "ROLLBACK",
            b'T',
        )
        .await;
        let restored = pair(&mut writer, b'T').await;
        command(&mut writer, ADVANCE, "UPDATE 1", b'T').await;
        let before_commit = pair(&mut reader, b'I').await;
        command(&mut writer, "COMMIT", "COMMIT", b'I').await;
        let published = pair(&mut reader, b'I').await;

        // Assert
        assert_eq!(provisional, expected("shipped", "99"));
        for (frames, code) in [(&duplicate, "23505"), (&blocked, "25P02")] {
            assert_eq!(wire::error_code(frames).as_deref(), Some(code));
            assert_eq!(ready_status(frames), Some(b'E'));
            assert_eq!(command_tag(frames), None);
        }
        assert_eq!(restored, expected("shipped", "10"));
        assert_eq!(before_commit, expected("pending", "10"));
        assert_eq!(published, expected("shipped", "11"));
        close_connection(writer).await;
        close_connection(reader).await;
        server.stop().await;
    });
    run(async {
        let reopened = open();
        reopened.startup().expect("restart Cassie");
        let server = wire::spawn_server(reopened).await;
        let mut reader = connect(&server).await;
        assert_eq!(pair(&mut reader, b'I').await, expected("shipped", "11"));
        close_connection(reader).await;
        server.stop().await;
    });
}
