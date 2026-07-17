use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use cassie::types::Value;
use uuid::Uuid;

#[path = "support/pgwire.rs"]
mod wire;

fn data_dir(label: &str) -> String {
    std::env::temp_dir()
        .join(format!("cassie-query-controls-{label}-{}", Uuid::new_v4()))
        .to_string_lossy()
        .into_owned()
}

fn configured_cassie(label: &str, memory_budget: usize) -> (Cassie, String) {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = data_dir(label);
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
    cassie.startup().expect("startup");
    (cassie, path)
}

fn seed_documents(cassie: &Cassie, table: &str, count: usize, payload_size: usize) {
    let rows = (0..count)
        .map(|index| {
            (
                Some(format!("doc-{index:04}")),
                serde_json::json!({
                    "payload": format!("{index:04}-{}", "x".repeat(payload_size)),
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(table, rows)
        .expect("seed documents");
}

fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field, _)| *field == tag)
        .map(|(_, value)| value.as_str())
}

#[test]
fn should_reject_unbounded_scan_without_partial_rows_given_low_memory_budget() {
    // Arrange
    let (cassie, path) = configured_cassie("low-scan-budget", 512);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_scan_budget (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_scan_budget", 32, 256);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM controlled_scan_budget",
            vec![],
        )
        .expect_err("unbounded scan should exceed the query budget");
    let metrics = cassie.metrics();

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        metrics["query"]["errors_by_class"]["resource_limit"].as_u64(),
        Some(1)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_stop_limit_scan_before_low_memory_budget_is_exhausted() {
    // Arrange
    let (cassie, path) = configured_cassie("limit-early-stop", 512);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_limit_scan (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_limit_scan", 64, 1_024);
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM controlled_limit_scan LIMIT 1",
            vec![],
        )
        .expect("LIMIT should avoid retaining the complete scan");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert_eq!(visited, 1, "LIMIT 1 must consume one native row");
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_stop_exists_scan_after_first_inner_row_given_low_memory_budget() {
    // Arrange
    let (cassie, path) = configured_cassie("exists-early-stop", 768);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_exists_outer (payload TEXT)",
            vec![],
        )
        .expect("create outer table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_exists_inner (payload TEXT)",
            vec![],
        )
        .expect("create inner table");
    seed_documents(&cassie, "controlled_exists_outer", 1, 16);
    seed_documents(&cassie, "controlled_exists_inner", 64, 1_024);
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM controlled_exists_outer WHERE EXISTS (SELECT id FROM controlled_exists_inner)",
            vec![],
        )
        .expect("EXISTS should stop after the first inner row");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        visited, 2,
        "outer and inner scans should each consume one row"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_transaction_overlay_visibility_under_query_controls() {
    // Arrange
    let (cassie, path) = configured_cassie("transaction-overlay", 8 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_overlay_visibility (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_overlay_visibility", 1, 16);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO controlled_overlay_visibility (payload) VALUES ('staged')",
            vec![],
        )
        .expect("stage insert");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM controlled_overlay_visibility ORDER BY payload",
            vec![],
        )
        .expect("overlay query");

    // Assert
    assert_eq!(result.rows.len(), 2);
    assert!(result
        .rows
        .contains(&vec![Value::String("staged".to_string())]));
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback transaction");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bound_native_reads_for_limit_with_transaction_overlay() {
    // Arrange
    let (cassie, path) = configured_cassie("overlay-limit", 16 * 1_024 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_overlay_limit (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_overlay_limit", 64, 64);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO controlled_overlay_limit (payload) VALUES ('staged')",
            vec![],
        )
        .expect("stage insert");
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM controlled_overlay_limit LIMIT 1",
            vec![],
        )
        .expect("bounded overlay query");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        visited, 1,
        "overlay LIMIT must not clone the persisted collection"
    );
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback transaction");

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_join_budget_failure_with_program_limit_sqlstate() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = data_dir("join-sqlstate");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = 4 * 1_024;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_join_left (payload TEXT)",
                vec![],
            )
            .expect("create left table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_join_right (payload TEXT)",
                vec![],
            )
            .expect("create right table");
        seed_documents(&cassie, "controlled_join_left", 16, 48);
        seed_documents(&cassie, "controlled_join_right", 16, 48);
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        // Act
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT controlled_join_left.payload, controlled_join_right.payload FROM controlled_join_left CROSS JOIN controlled_join_right",
            )],
        )
        .await;
        let frames = wire::read_frames_until_ready(&mut reader).await;

        // Assert
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("join resource error");
        let fields = wire::parse_error_fields(&error.1);
        assert_eq!(error_field(&fields, 'C'), Some("54000"));
        assert!(error_field(&fields, 'M')
            .expect("error message")
            .contains("query memory budget exceeded"));

        server.stop().await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_stop_cross_join_after_limit_without_materializing_both_inputs() {
    // Arrange
    let (cassie, path) = configured_cassie("cross-join-limit", 8 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_cross_left (payload TEXT)",
            vec![],
        )
        .expect("create left table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_cross_right (payload TEXT)",
            vec![],
        )
        .expect("create right table");
    seed_documents(&cassie, "controlled_cross_left", 64, 1_024);
    seed_documents(&cassie, "controlled_cross_right", 64, 1_024);
    let before = cassie.midge.query_scan_entries_for_diagnostics();

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT controlled_cross_left.payload, controlled_cross_right.payload FROM controlled_cross_left CROSS JOIN controlled_cross_right LIMIT 1",
            vec![],
        )
        .expect("LIMIT should bound both cross-join inputs and output");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert_eq!(result.rows.len(), 1);
    assert!(
        visited <= 2,
        "LIMIT 1 cross join should consume at most one row from each input, visited {visited}"
    );
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cancel_at_a_deterministic_mid_scan_boundary_without_leaking_reservations() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let (cassie, path) = configured_cassie("deterministic-cancellation", 64 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_mid_scan_cancel (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_mid_scan_cancel", 64, 128);
    let before = cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(3));

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM controlled_mid_scan_cancel",
            vec![],
        )
        .expect_err("deterministic scan hook should cancel the query");
    set_query_scan_cancellation_after_entries(None);
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);
    let metrics = cassie.metrics();

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(visited, 3);
    assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
    assert_eq!(
        metrics["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_cancel_wildcard_scan_at_the_same_controlled_storage_boundary() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let (cassie, path) = configured_cassie("wildcard-cancellation", 64 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_wildcard_cancel (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_wildcard_cancel", 64, 128);
    let before = cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(4));

    // Act
    let error = cassie
        .execute_sql(&session, "SELECT * FROM controlled_wildcard_cancel", vec![])
        .expect_err("wildcard scan should observe the controlled cursor cancellation");
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(visited, 4);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(path);
}
