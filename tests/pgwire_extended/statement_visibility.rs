//! Finite statement acquisition and suspended-owner boundary controls.
use super::support_pgwire as wire;
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

type Frames = Vec<(u8, Vec<u8>)>;
async fn round_trip(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    frames: Vec<Vec<u8>>,
) -> Frames {
    wire::write_frames(writer, frames).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}
fn fixture(path: &str) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("setup", None);
    cassie
        .execute_sql(&session, "CREATE TABLE visibility_rows (n INT)", vec![])
        .expect("table");
    for n in [10, 20, 30] {
        cassie
            .midge
            .put_document(
                "visibility_rows",
                Some(format!("doc-{n}")),
                serde_json::json!({"n":n}),
            )
            .expect("seed");
    }
    cassie
}
fn values(frames: &[(u8, Vec<u8>)]) -> Vec<String> {
    assert_eq!(
        wire::error_code(frames),
        None,
        "statement visibility fixture must not return an error SQLSTATE"
    );
    wire::data_rows(frames)
        .into_iter()
        .map(|row| row.into_iter().next().flatten().expect("n"))
        .collect()
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn should_capture_portal_data_at_first_execute_after_bind() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("statement-first-execute");
    runtime().block_on(async {
        let cassie = fixture(&path);
        let server = wire::spawn_server(cassie.clone()).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::transaction_control(&mut reader, &mut writer, "BEGIN").await;
        let bound = round_trip(
            &mut reader,
            &mut writer,
            vec![
                wire::parse_frame("source", "SELECT n FROM visibility_rows ORDER BY n"),
                wire::bind_frame("rows", "source", &[]),
                wire::sync_frame(),
            ],
        )
        .await;
        assert_eq!(wire::error_code(&bound), None);
        assert_eq!(wire::data_rows(&bound), Vec::<Vec<Option<String>>>::new());
        let publisher = cassie.create_session("publisher", None);
        cassie
            .execute_sql(
                &publisher,
                "INSERT INTO visibility_rows (n) VALUES (40)",
                vec![],
            )
            .expect("commit after Bind before Execute");
        // Act
        let first = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::execute_frame("rows"), wire::sync_frame()],
        )
        .await;
        let closed = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::close_portal_frame("rows"), wire::sync_frame()],
        )
        .await;
        // Assert
        assert_eq!(values(&first), ["10", "20", "30", "40"]);
        assert_eq!(wire::error_code(&closed), None);
        assert_eq!(cassie.metrics()["pgwire"]["portals"].as_u64(), Some(0));
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        drop((reader, writer));
        server.stop().await;
    });
    std::fs::remove_dir_all(path).expect("fixture cleanup");
}

#[test]
fn should_resume_suspended_portal_after_staged_savepoint_rollback() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("statement-resume-savepoint");
    runtime().block_on(async {
        let cassie = fixture(&path);
        let server = wire::spawn_server(cassie.clone()).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::transaction_control(&mut reader, &mut writer, "BEGIN").await;
        let saved = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::simple_query_frame("SAVEPOINT before_rows")],
        )
        .await;
        assert_eq!(wire::error_code(&saved), None);
        let first = round_trip(
            &mut reader,
            &mut writer,
            vec![
                wire::parse_frame("source", "SELECT n FROM visibility_rows"),
                wire::bind_frame("rows", "source", &[]),
                wire::execute_limited_frame("rows", 1),
                wire::sync_frame(),
            ],
        )
        .await;
        assert_eq!(values(&first), ["10"]);
        assert!(first.iter().any(|frame| frame.0 == b's'));
        let staged = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::simple_query_frame(
                "INSERT INTO visibility_rows (n) VALUES (99)",
            )],
        )
        .await;
        assert_eq!(wire::error_code(&staged), None);
        // Act
        let rolled_back = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::simple_query_frame("ROLLBACK TO before_rows")],
        )
        .await;
        let resumed = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::execute_limited_frame("rows", 10), wire::sync_frame()],
        )
        .await;
        let closed = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::close_portal_frame("rows"), wire::sync_frame()],
        )
        .await;
        // Assert
        assert_eq!(wire::error_code(&rolled_back), None);
        assert_eq!(values(&resumed), ["20", "30"]);
        assert_eq!(wire::error_code(&closed), None);
        assert_eq!(cassie.metrics()["pgwire"]["portals"].as_u64(), Some(0));
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        assert_eq!(
            cassie.metrics()["runtime"]["active_operator_workers"].as_u64(),
            Some(0)
        );
        drop((reader, writer));
        server.stop().await;
    });
    std::fs::remove_dir_all(path).expect("fixture cleanup");
}

#[test]
fn should_release_a_suspended_portal_owner_on_close() {
    // Arrange
    wire::use_local_storage();
    let path = wire::data_dir("statement-suspended-close");
    runtime().block_on(async {
        let cassie = fixture(&path);
        let server = wire::spawn_server(cassie.clone()).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;
        wire::transaction_control(&mut reader, &mut writer, "BEGIN").await;
        let first = round_trip(
            &mut reader,
            &mut writer,
            vec![
                wire::parse_frame("source", "SELECT n FROM visibility_rows"),
                wire::bind_frame("rows", "source", &[]),
                wire::execute_limited_frame("rows", 1),
                wire::sync_frame(),
            ],
        )
        .await;
        assert_eq!(values(&first), ["10"]);
        assert!(first.iter().any(|frame| frame.0 == b's'));
        assert_eq!(cassie.metrics()["pgwire"]["portals"].as_u64(), Some(1));
        // Act
        let closed = round_trip(
            &mut reader,
            &mut writer,
            vec![wire::close_portal_frame("rows"), wire::sync_frame()],
        )
        .await;
        let metrics = cassie.metrics();
        // Assert
        assert_eq!(wire::error_code(&closed), None);
        assert!(closed.iter().any(|frame| frame.0 == b'3'));
        assert_eq!(metrics["pgwire"]["portals"].as_u64(), Some(0));
        assert_eq!(
            metrics["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        assert_eq!(
            metrics["runtime"]["active_operator_workers"].as_u64(),
            Some(0)
        );
        drop((reader, writer));
        server.stop().await;
    });
    std::fs::remove_dir_all(path).expect("fixture cleanup");
}
