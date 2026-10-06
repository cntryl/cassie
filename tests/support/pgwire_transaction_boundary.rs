//! Independent-session visibility at the selected extended transaction boundary.

use crate::support_pgwire as wire;
use cassie::app::Cassie;
use std::time::Duration;

pub type Frames = Vec<(u8, Vec<u8>)>;

#[derive(Clone, Copy)]
pub enum Prefix {
    Idle,
    Implicit,
    ImplicitParentConflict,
    Explicit,
    ExplicitOutputError,
}

pub struct Transcript {
    pub prefix: Frames,
    pub before: Frames,
    pub boundary: Frames,
    pub after: Frames,
    pub tail: Vec<Frames>,
    pub terminal: Frames,
}

fn fixture(path: &str, prefix: Prefix) -> Cassie {
    let parent_conflict = matches!(prefix, Prefix::ImplicitParentConflict);
    let output_error = matches!(prefix, Prefix::ExplicitOutputError);
    let cassie = if output_error {
        let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = 512 * 1024 * 1024;
        Cassie::new_with_data_dir_and_config(path, config).expect("Cassie with output probe budget")
    } else {
        Cassie::new_with_data_dir(path).expect("Cassie")
    };
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let table_sql = if parent_conflict {
        for sql in [
            "CREATE TABLE wire_parent (n INT PRIMARY KEY)",
            "INSERT INTO wire_parent (n) VALUES (1)",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed parent before tested cycle");
        }
        "CREATE TABLE wire_tx (n INT REFERENCES wire_parent(n))"
    } else if output_error {
        "CREATE TABLE wire_tx (n INT, payload TEXT)"
    } else {
        "CREATE TABLE wire_tx (n INT)"
    };
    cassie
        .execute_sql(&session, table_sql, Vec::new())
        .expect("create fixture separately from tested cycle");
    if output_error {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO wire_tx (n, payload) VALUES (0, $1)",
                vec![cassie::types::Value::String("x".repeat(1024 * 1024))],
            )
            .expect("seed one admitted payload before tested cycle");
    }

    cassie
}

async fn exchange(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    frames: Vec<Vec<u8>>,
) -> Frames {
    wire::write_frames(writer, frames).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

async fn query(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    sql: &str,
) -> Frames {
    exchange(reader, writer, vec![wire::simple_query_frame(sql)]).await
}

pub fn run_boundary(
    boundary: Vec<Vec<u8>>,
    observer_sql: &str,
) -> (Frames, Frames, Frames, Frames) {
    run_cycle(boundary, observer_sql, true)
}

pub fn run_cycle(
    boundary: Vec<Vec<u8>>,
    observer_sql: &str,
    extended_prefix: bool,
) -> (Frames, Frames, Frames, Frames) {
    let prefix = if extended_prefix {
        Prefix::Implicit
    } else {
        Prefix::Idle
    };
    let transcript = run_script(prefix, boundary, observer_sql, Vec::new());
    (
        transcript.prefix,
        transcript.before,
        transcript.boundary,
        transcript.after,
    )
}

pub fn run_script(
    prefix: Prefix,
    boundary: Vec<Vec<u8>>,
    observer_sql: &str,
    tail: Vec<Vec<Vec<u8>>>,
) -> Transcript {
    let parent_conflict = matches!(prefix, Prefix::ImplicitParentConflict);
    wire::use_local_storage();
    let path = wire::data_dir("extended-transaction-privacy");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    let transcript = runtime.block_on(async {
        let cassie = fixture(&path, prefix);
        let server = wire::spawn_server(cassie).await;
        let owner = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("owner connection");
        let observer = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("independent observer connection");
        let (mut owner_read, mut owner_write) = tokio::io::split(owner);
        let (mut observer_read, mut observer_write) = tokio::io::split(observer);
        wire::complete_startup(&mut owner_read, &mut owner_write).await;
        wire::complete_startup(&mut observer_read, &mut observer_write).await;
        if matches!(prefix, Prefix::Explicit | Prefix::ExplicitOutputError) {
            wire::transaction_control(&mut owner_read, &mut owner_write, "BEGIN").await;
        }

        // Act
        let prefix = if matches!(prefix, Prefix::Idle) {
            Vec::new()
        } else {
            wire::write_frames(
                &mut owner_write,
                vec![
                    wire::parse_frame("prefix_s", "INSERT INTO wire_tx (n) VALUES (1)"),
                    wire::bind_frame("prefix_p", "prefix_s", &[]),
                    wire::execute_frame("prefix_p"),
                    wire::flush_frame(),
                ],
            )
            .await;
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut frames = Vec::new();
                for _ in 0..3 {
                    frames.push(wire::read_wire_frame(&mut owner_read).await);
                }
                frames
            })
            .await
            .expect("Execute must complete before observer query")
        };
        let before = query(
            &mut observer_read,
            &mut observer_write,
            "SELECT COUNT(*) FROM wire_tx",
        )
        .await;
        if parent_conflict {
            wire::write_frames(
                &mut observer_write,
                vec![wire::simple_query_frame(
                    "DELETE FROM wire_parent WHERE n = 1",
                )],
            )
            .await;
            let deleted =
                wire::read_frames_until_ready_within(&mut observer_read, Duration::from_secs(5))
                    .await;
            assert_eq!(wire::error_code(&deleted), None);
            assert_eq!(
                deleted.iter().map(|frame| frame.0).collect::<Vec<_>>(),
                b"CZ"
            );
            assert_eq!(deleted[0].1, b"DELETE 1\0");
        }
        let boundary = exchange(&mut owner_read, &mut owner_write, boundary).await;
        let after = query(&mut observer_read, &mut observer_write, observer_sql).await;
        let mut tail_results = Vec::new();
        for cycle in tail {
            tail_results.push(exchange(&mut owner_read, &mut owner_write, cycle).await);
        }
        let terminal = if tail_results.is_empty() {
            Vec::new()
        } else {
            query(&mut observer_read, &mut observer_write, observer_sql).await
        };
        drop((owner_read, owner_write, observer_read, observer_write));
        server.stop().await;
        Transcript {
            prefix,
            before,
            boundary,
            after,
            tail: tail_results,
            terminal,
        }
    });
    let _ = std::fs::remove_dir_all(path);
    transcript
}

pub fn copy_access_fixture(path: &str) -> (Cassie, cassie::app::CassieSession) {
    let cassie = Cassie::new_with_data_dir(path).expect("Cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some("postgres"), None)
        .expect("admin");
    for sql in [
        "CREATE TABLE wire_copy_denied (n INT)",
        "CREATE ROLE boundary_reader LOGIN PASSWORD 'reader-secret'",
        "GRANT CONNECT ON DATABASE postgres TO boundary_reader",
    ] {
        cassie
            .execute_sql(&admin, sql, Vec::new())
            .expect("fixture");
    }
    (cassie, admin)
}
