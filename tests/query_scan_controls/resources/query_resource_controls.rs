use super::support_pgwire as wire;
use cassie::app::{Cassie, CassieError};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::executor::projection::set_projection_build_failure_point;
use cassie::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
};
use cassie::types::Value;

fn configured_cassie(memory_budget: usize) -> Cassie {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("configured cassie");
    cassie.startup().expect("startup");
    cassie
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

fn assert_failed_query_after_source_scan(
    cassie: &Cassie,
    before_metrics: &serde_json::Value,
    before_entries: u64,
    expected_entries: u64,
) {
    assert_eq!(
        cassie.midge.query_scan_entries_for_diagnostics() - before_entries,
        expected_entries,
        "resource rejection must follow complete source materialization"
    );
    let after = cassie.metrics();
    assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
    assert_eq!(after["runtime"]["running_queries"], 0);
    assert_eq!(after["runtime"]["active_operator_workers"], 0);
    assert_eq!(
        after["query"]["rows_returned_total"], before_metrics["query"]["rows_returned_total"],
        "failed operators must not publish successful rows"
    );
}

fn assert_join_success_was_not_published(
    before_metrics: &serde_json::Value,
    after_metrics: &serde_json::Value,
) {
    assert_eq!(
        after_metrics["joins"], before_metrics["joins"],
        "a rejected joined output must not publish successful join diagnostics"
    );
}

#[test]
fn should_reject_unbounded_scan_without_partial_rows_given_low_memory_budget() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(512);
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
}

#[test]
fn should_stop_limit_scan_before_low_memory_budget_is_exhausted() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    // Admit the 704-byte statement owner before testing the one-row stop.
    let cassie = configured_cassie(4_096);
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
    assert_eq!(result.rows().len(), 1);
    assert_eq!(visited, 1, "LIMIT 1 must consume one native row");
    drop(result);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_stop_exists_scan_after_first_inner_row_given_low_memory_budget() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    // The memory backend needs 3,170 bytes for the first-row EXISTS path; a
    // full inner scan still exceeds this 4 KiB budget.
    let cassie = configured_cassie(4 * 1024);
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
    let full_inner = cassie
        .execute_sql(&session, "SELECT id FROM controlled_exists_inner", vec![])
        .expect_err("full inner scan exceeds the same bounded budget");
    assert!(matches!(full_inner, CassieError::ResourceLimit(_)));
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
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
    assert_eq!(result.rows().len(), 1);
    assert_eq!(
        visited, 2,
        "outer and inner scans should each consume one row"
    );
}

#[test]
fn should_preserve_transaction_overlay_visibility_under_query_controls() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    // Captured transaction overlay and sort/projection owners share the budget.
    // This positive visibility fixture uses a finite 32 KiB allowance;
    // separate denial controls retain their selected low-memory frontiers.
    let cassie = configured_cassie(32 * 1_024);
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
    assert_eq!(result.rows().len(), 2);
    assert!(result
        .rows()
        .contains(&vec![Value::String("staged".to_string())]));
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback transaction");
}

#[test]
fn should_bound_native_reads_for_limit_with_transaction_overlay() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(16 * 1_024 * 1_024);
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
    assert_eq!(result.rows().len(), 1);
    assert_eq!(
        visited, 1,
        "overlay LIMIT must not clone the persisted collection"
    );
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback transaction");
}

#[test]
fn should_report_join_budget_failure_with_program_limit_sqlstate() {
    // Arrange
    const FIXTURE_BUDGET_BYTES: usize = 256 * 1_024;
    let _hook_guard = query_scan_control_test_guard();
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    // Qualified retained inputs fit; the 256-row joined output exceeds the shared cap.
    config.limits.query_memory_budget_bytes = FIXTURE_BUDGET_BYTES;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
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
        for table in ["controlled_join_left", "controlled_join_right"] {
            let input = cassie
                .execute_sql(&session, &format!("SELECT payload FROM {table}"), vec![])
                .expect("each complete join input must fit the shared query budget");
            assert_eq!(input.rows().len(), 16);
        }
        let before_positive_entries = cassie.midge.query_scan_entries_for_diagnostics();
        let positive = cassie
            .execute_sql(
                &session,
                "SELECT controlled_join_left.payload, controlled_join_right.payload FROM controlled_join_left INNER JOIN controlled_join_right ON FALSE",
                vec![],
            )
            .expect("both qualified retained inputs and a rejected candidate must fit");
        assert_eq!(positive.rows(), Vec::<Vec<Value>>::new());
        assert_eq!(
            cassie.midge.query_scan_entries_for_diagnostics() - before_positive_entries,
            32
        );
        drop(positive);
        let positive_metrics = cassie.metrics();
        assert_eq!(positive_metrics["joins"]["last_strategy"], "nested_loop");
        let positive_peak = positive_metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .expect("positive input admission peak");
        assert!(positive_peak <= u64::try_from(FIXTURE_BUDGET_BYTES).expect("fixture budget"));
        assert_eq!(positive_metrics["query"]["current_accounted_memory_bytes"], 0);
        eprintln!("qualified loaded join input admission peak: {positive_peak} bytes");
        let sql = "SELECT controlled_join_left.payload, controlled_join_right.payload FROM controlled_join_left CROSS JOIN controlled_join_right";
        let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
        let before_metrics = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(&session, sql, vec![])
            .expect_err("joined state must exceed the budget after loading both inputs");
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_failed_query_after_source_scan(&cassie, &before_metrics, before_entries, 32);
        assert_join_success_was_not_published(&before_metrics, &cassie.metrics());
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(sql)],
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
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());

        server.stop().await;
    });
}

#[test]
fn should_stop_cross_join_after_limit_without_materializing_both_inputs() {
    // Arrange
    const FIXTURE_BUDGET_BYTES: usize = 32 * 1_024;
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(FIXTURE_BUDGET_BYTES);
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
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(result.rows().len(), 1);
    assert!(
        visited <= 2,
        "LIMIT 1 cross join should consume at most one row from each input, visited {visited}"
    );
    assert_eq!(metrics["joins"]["last_strategy"], "cross");
    assert_eq!(metrics["joins"]["executions"], 1);
    assert_eq!(metrics["joins"]["left_input_rows_total"], 1);
    assert_eq!(metrics["joins"]["right_input_rows_total"], 1);
    assert_eq!(metrics["joins"]["matched_rows_total"], 1);
    assert_eq!(metrics["joins"]["output_rows_total"], 1);
    let peak = metrics["query"]["peak_accounted_memory_bytes"]
        .as_u64()
        .expect("accounted memory peak");
    assert!(peak <= u64::try_from(FIXTURE_BUDGET_BYTES).expect("fixture memory budget"));
    assert_eq!(metrics["runtime"]["running_queries"], 0);
    assert_eq!(metrics["runtime"]["active_operator_workers"], 0);
    drop(result);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    println!("bounded CROSS accounted memory peak: {peak} bytes; native entries: {visited}");
}

#[test]
fn should_cancel_at_a_deterministic_mid_scan_boundary_without_leaking_reservations() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(64 * 1_024);
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
}

#[test]
fn should_cancel_unindexed_heap_top_k_at_controlled_scan_boundary() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(64 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_heap_top_k_cancel (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_heap_top_k_cancel", 64, 128);
    let before = cassie.midge.query_scan_entries_for_diagnostics();
    set_query_scan_cancellation_after_entries(Some(3));

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM controlled_heap_top_k_cancel ORDER BY payload LIMIT 5",
            vec![],
        )
        .expect_err("heap top-k should observe controlled cancellation");
    set_query_scan_cancellation_after_entries(None);
    let visited = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(visited, 3);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_reject_unindexed_heap_top_k_before_exceeding_memory_budget() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(10 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_heap_top_k_memory (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_heap_top_k_memory", 4, 80);
    let input = cassie
        .execute_sql(
            &session,
            "SELECT * FROM controlled_heap_top_k_memory",
            vec![],
        )
        .expect("complete top-k input must fit before ordering-key retention");
    assert_eq!(input.rows().len(), 4);
    drop(input);
    // A computed order declines the simple-column shortcut. Keep metadata
    // width narrow while two computed keys contain 2 * 64 * 85 TEXT bytes,
    // exceeding 10 KiB after the complete four-row input fits.
    let key_arguments = vec!["payload"; 64].join(", ");
    let sql = format!(
        "SELECT payload FROM controlled_heap_top_k_memory ORDER BY concat({key_arguments}) LIMIT 2"
    );
    let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
    let before_metrics = cassie.metrics();

    // Act
    let error = cassie
        .execute_sql(&session, &sql, vec![])
        .expect_err("heap top-k should respect query memory budget");

    // Assert
    assert!(matches!(error, CassieError::ResourceLimit(_)));
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_failed_query_after_source_scan(&cassie, &before_metrics, before_entries, 4);
}

#[test]
fn should_reject_expanding_projection_before_building_output() {
    let _hook_guard = query_scan_control_test_guard();
    // Arrange
    let cassie = configured_cassie(4 * 1_024);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE controlled_expanding_projection (payload TEXT)",
            vec![],
        )
        .expect("create table");
    seed_documents(&cassie, "controlled_expanding_projection", 1, 256);
    let input = cassie
        .execute_sql(
            &session,
            "SELECT * FROM controlled_expanding_projection",
            vec![],
        )
        .expect("projection input must fit before output expansion");
    assert_eq!(input.rows().len(), 1);
    // Keep source decode/handoff below the cap while reserving sixteen payload copies.
    let projection_arguments = vec!["payload"; 16].join(", ");
    let sql = format!(
        "SELECT concat({projection_arguments}) AS expanded FROM controlled_expanding_projection"
    );
    let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
    let before_metrics = cassie.metrics();
    set_projection_build_failure_point(true);

    // Act
    let error = cassie
        .execute_sql(&session, &sql, vec![])
        .expect_err("projection should reserve expansion before building");
    set_projection_build_failure_point(false);

    // Assert
    assert!(
        matches!(error, CassieError::ResourceLimit(_)),
        "unexpected expanding projection error: {error:?}"
    );
    assert!(error.to_string().contains("query memory budget exceeded"));
    assert_failed_query_after_source_scan(&cassie, &before_metrics, before_entries, 1);
}

#[test]
fn should_cancel_wildcard_scan_at_the_same_controlled_storage_boundary() {
    // Arrange
    let _hook_guard = query_scan_control_test_guard();
    let cassie = configured_cassie(64 * 1_024);
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
}
