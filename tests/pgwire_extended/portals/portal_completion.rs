use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use tokio::io::BufReader;

use super::support_pgwire as support;

type Frames = Vec<(u8, Vec<u8>)>;
type PortalPages = (Vec<String>, Vec<String>);

fn seeded_cassie(path: &str, table: &str, count: usize) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {table} (payload TEXT)"),
            vec![],
        )
        .expect("create table");
    for index in 0..count {
        cassie
            .midge
            .put_document(
                table,
                Some(format!("doc-{index:02}")),
                serde_json::json!({"payload": format!("value-{index:02}")}),
            )
            .expect("seed row");
    }
    cassie
}

fn data_values(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .filter_map(|(_, payload)| {
            support::parse_data_row(payload)
                .into_iter()
                .next()
                .flatten()
        })
        .collect()
}

fn command_tags(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload)
                .trim_end_matches('\0')
                .to_string()
        })
        .collect()
}

fn error_code(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    let (_, payload) = frames.iter().find(|(tag, _)| *tag == b'E')?;
    support::parse_error_fields(payload)
        .into_iter()
        .find_map(|(tag, value)| (tag == 'C').then_some(value))
}

async fn round_trip(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
    writer: &mut tokio::net::tcp::WriteHalf<'_>,
    frames: Vec<Vec<u8>>,
) -> Frames {
    support::write_frames(writer, frames).await;
    support::read_frames_until_ready(reader).await
}

/// Opens a transaction, suspends a streaming portal after two rows, stages
/// `staged_sql` in the same transaction, and resumes the portal.
fn resume_after_staged_write(label: &str, staged_sql: &str) -> PortalPages {
    support::use_local_storage();
    let path = support::data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let pages = runtime.block_on(async {
        let cassie = seeded_cassie(&path, "portal_staged_resume", 6);
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        let begin = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("BEGIN")],
        )
        .await;
        assert!(begin.iter().all(|(tag, _)| *tag != b'E'));
        let first = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::parse_frame("resume_stmt", "SELECT payload FROM portal_staged_resume"),
                support::bind_frame("resume_portal", "resume_stmt", &[]),
                support::execute_limited_frame("resume_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        assert!(first.iter().any(|(tag, _)| *tag == b's'), "{first:?}");
        let staged = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame(staged_sql)],
        )
        .await;
        assert!(
            staged.iter().all(|(tag, _)| *tag != b'E'),
            "{staged_sql} failed"
        );
        let second = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::execute_limited_frame("resume_portal", 100),
                support::sync_frame(),
            ],
        )
        .await;
        assert!(second.iter().all(|(tag, _)| *tag != b'E'), "{second:?}");
        drop(socket);
        server.stop().await;
        (data_values(&first), data_values(&second))
    });
    let _ = std::fs::remove_dir_all(path);
    pages
}

fn original_rows() -> Vec<String> {
    (0..6).map(|index| format!("value-{index:02}")).collect()
}

#[test]
fn should_resume_suspended_portal_from_original_result_after_staged_insert() {
    // Arrange
    let staged_sql = "INSERT INTO portal_staged_resume (payload) VALUES ('staged')";

    // Act
    let (first, second) = resume_after_staged_write("portal-staged-insert", staged_sql);

    // Assert
    assert_eq!(first, original_rows()[..2].to_vec());
    assert_eq!(second, original_rows()[2..].to_vec());
}

#[test]
fn should_resume_suspended_portal_from_original_result_after_staged_delete() {
    // Arrange
    let staged_sql = "DELETE FROM portal_staged_resume WHERE payload = 'value-00'";

    // Act
    let (first, second) = resume_after_staged_write("portal-staged-delete", staged_sql);

    // Assert
    assert_eq!(first, original_rows()[..2].to_vec());
    assert_eq!(second, original_rows()[2..].to_vec());
}

#[test]
fn should_return_select_zero_when_completed_select_portal_executes_again() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-completed-select");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = seeded_cassie(&path, "portal_completed_select", 3);
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        // Cross-Sync portals belong to an explicit transaction.
        support::transaction_control(&mut reader, &mut write_half, "BEGIN").await;
        let first = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::parse_frame(
                    "completed_select",
                    "SELECT payload FROM portal_completed_select",
                ),
                support::bind_frame("completed_select_portal", "completed_select", &[]),
                support::execute_frame("completed_select_portal"),
                support::sync_frame(),
            ],
        )
        .await;
        assert_eq!(data_values(&first).len(), 3);

        // Act
        let again = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::execute_frame("completed_select_portal"),
                support::sync_frame(),
            ],
        )
        .await;

        // Assert
        assert!(data_values(&again).is_empty(), "{again:?}");
        assert_eq!(command_tags(&again), vec!["SELECT 0".to_string()]);

        drop(socket);
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_not_rerun_insert_when_completed_insert_portal_executes_again() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-completed-insert");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = seeded_cassie(&path, "portal_completed_insert", 0);
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        // Cross-Sync portals belong to an explicit transaction.
        support::transaction_control(&mut reader, &mut write_half, "BEGIN").await;
        let first = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::parse_frame(
                    "completed_insert",
                    "INSERT INTO portal_completed_insert (payload) VALUES ('once')",
                ),
                support::bind_frame("completed_insert_portal", "completed_insert", &[]),
                support::execute_frame("completed_insert_portal"),
                support::sync_frame(),
            ],
        )
        .await;
        assert_eq!(command_tags(&first), vec!["INSERT 0 1".to_string()]);

        // Act
        let again = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::execute_frame("completed_insert_portal"),
                support::sync_frame(),
            ],
        )
        .await;
        let selected = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame(
                "SELECT payload FROM portal_completed_insert",
            )],
        )
        .await;

        // Assert
        assert_eq!(command_tags(&again), vec!["INSERT 0 0".to_string()]);
        assert_eq!(data_values(&selected), vec!["once".to_string()]);

        drop(socket);
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_drop_suspended_portal_when_extended_commit_ends_transaction() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-extended-commit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = seeded_cassie(&path, "portal_extended_commit", 4);
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        let _ = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("BEGIN")],
        )
        .await;
        let first = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::parse_frame(
                    "commit_select",
                    "SELECT payload FROM portal_extended_commit",
                ),
                support::bind_frame("commit_portal", "commit_select", &[]),
                support::execute_limited_frame("commit_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        assert!(first.iter().any(|(tag, _)| *tag == b's'), "{first:?}");
        let committed = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::parse_frame("commit_stmt", "COMMIT"),
                support::bind_frame("commit_stmt_portal", "commit_stmt", &[]),
                support::execute_frame("commit_stmt_portal"),
                support::sync_frame(),
            ],
        )
        .await;
        assert!(
            committed.iter().all(|(tag, _)| *tag != b'E'),
            "{committed:?}"
        );

        // Act
        let resumed = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::execute_limited_frame("commit_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;

        // Assert
        assert!(data_values(&resumed).is_empty(), "{resumed:?}");
        assert_eq!(error_code(&resumed).as_deref(), Some("34000"));

        drop(socket);
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_drop_suspended_portal_given_commit_chain_when_next_transaction_remains_open() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("portal-extended-commit-chain");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = seeded_cassie(&path, "portal_extended_commit_chain", 4);
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        let _ = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("BEGIN")],
        )
        .await;
        let first = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::parse_frame(
                    "commit_select",
                    "SELECT payload FROM portal_extended_commit_chain",
                ),
                support::bind_frame("commit_portal", "commit_select", &[]),
                support::execute_limited_frame("commit_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;
        assert!(first.iter().any(|(tag, _)| *tag == b's'), "{first:?}");
        let committed = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::parse_frame("commit_stmt", "COMMIT AND CHAIN"),
                support::bind_frame("commit_stmt_portal", "commit_stmt", &[]),
                support::execute_frame("commit_stmt_portal"),
                support::sync_frame(),
            ],
        )
        .await;
        assert!(
            committed.iter().all(|(tag, _)| *tag != b'E'),
            "{committed:?}"
        );
        assert!(
            committed
                .iter()
                .any(|(tag, body)| *tag == b'Z' && body == b"T"),
            "replacement transaction must remain active: {committed:?}"
        );

        // Act
        let resumed = round_trip(
            &mut reader,
            &mut write_half,
            vec![
                support::execute_limited_frame("commit_portal", 2),
                support::sync_frame(),
            ],
        )
        .await;

        // Assert
        assert!(data_values(&resumed).is_empty(), "{resumed:?}");
        assert_eq!(error_code(&resumed).as_deref(), Some("34000"));

        drop(socket);
        server.stop().await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_default_characteristics_given_failed_transaction_when_commit_or_rollback_chains() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("failed-chain-characteristics");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = seeded_cassie(&path, "failed_chain_rows", 1);
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr).await.expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;
        for control in ["COMMIT AND CHAIN", "ROLLBACK AND CHAIN"] {
            let _ = round_trip(&mut reader, &mut write_half, vec![support::simple_query_frame("BEGIN ISOLATION LEVEL READ UNCOMMITTED READ ONLY DEFERRABLE")]).await;
            let failed = round_trip(&mut reader, &mut write_half, vec![support::simple_query_frame("SELECT 1/0")]).await;
            assert_eq!(error_code(&failed).as_deref(), Some("22012"));
            assert!(failed.iter().any(|(tag, body)| *tag == b'Z' && body == b"E"));

            // Act
            let chained = round_trip(&mut reader, &mut write_half, vec![support::simple_query_frame(control)]).await;
            let modes = round_trip(&mut reader, &mut write_half, vec![support::simple_query_frame("SHOW transaction_isolation; SHOW transaction_read_only; SHOW transaction_deferrable")]).await;
            let savepoint = round_trip(&mut reader, &mut write_half, vec![support::simple_query_frame("SAVEPOINT actual_chain")]).await;
            let ended = round_trip(&mut reader, &mut write_half, vec![support::simple_query_frame("ROLLBACK")]).await;

            // Assert
            assert!(chained.iter().any(|(tag, body)| *tag == b'C' && body == b"ROLLBACK\0"), "{control}: {chained:?}");
            assert!(chained.iter().any(|(tag, body)| *tag == b'Z' && body == b"T"), "{control}: {chained:?}");
            assert_eq!(data_values(&modes), ["read committed", "off", "off"], "{control}");
            assert!(savepoint.iter().all(|(tag, _)| *tag != b'E'), "{control}: {savepoint:?}");
            assert!(ended.iter().any(|(tag, body)| *tag == b'Z' && body == b"I"));
        }
        drop(socket);
        server.stop().await;
    });
    std::fs::remove_dir_all(path).expect("remove failed-chain fixture");
}

#[test]
fn should_report_transaction_status_given_implicit_begin_modes_when_prior_query_established_boundary(
) {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("implicit-begin-mode-boundary");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = seeded_cassie(&path, "implicit_begin_rows", 1);
        let server = support::spawn_server(cassie).await;
        let mut socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (read_half, mut write_half) = socket.split();
        let mut reader = BufReader::new(read_half);
        support::complete_startup(&mut reader, &mut write_half).await;

        // Act
        let rejected_begin = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame(
                "SELECT 1; BEGIN ISOLATION LEVEL READ UNCOMMITTED",
            )],
        )
        .await;
        let accepted_begin = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("SELECT 1; BEGIN READ ONLY")],
        )
        .await;
        let rejected_mode = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("SET transaction_read_only=off")],
        )
        .await;
        let aborted = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("SELECT 2")],
        )
        .await;
        let rolled_back = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("ROLLBACK")],
        )
        .await;
        let recovered = round_trip(
            &mut reader,
            &mut write_half,
            vec![support::simple_query_frame("SELECT 3")],
        )
        .await;

        // Assert
        assert_eq!(error_code(&rejected_begin).as_deref(), Some("25001"));
        assert!(rejected_begin
            .iter()
            .any(|(tag, body)| *tag == b'Z' && body == b"I"));
        assert!(
            accepted_begin.iter().all(|(tag, _)| *tag != b'E'),
            "{accepted_begin:?}"
        );
        assert!(accepted_begin
            .iter()
            .any(|(tag, body)| *tag == b'Z' && body == b"T"));
        assert_eq!(error_code(&rejected_mode).as_deref(), Some("25001"));
        assert!(rejected_mode
            .iter()
            .any(|(tag, body)| *tag == b'Z' && body == b"E"));
        assert_eq!(error_code(&aborted).as_deref(), Some("25P02"));
        assert!(aborted
            .iter()
            .any(|(tag, body)| *tag == b'Z' && body == b"E"));
        assert!(rolled_back
            .iter()
            .any(|(tag, body)| *tag == b'Z' && body == b"I"));
        assert_eq!(data_values(&recovered), ["3"]);
        assert!(recovered
            .iter()
            .any(|(tag, body)| *tag == b'Z' && body == b"I"));
        drop(socket);
        server.stop().await;
    });
    std::fs::remove_dir_all(path).expect("remove implicit-BEGIN fixture");
}
