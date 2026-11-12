//! Cancellation must stop Boolean aggregate input before publishing output.

use crate::support_boolean_aggregate_cancellation::{
    assert_case, current_thread_runtime, run_case,
};

async fn run_case(setup: &[&str], source: &str) -> CancellationCase {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("runtime config"),
    );
    config.limits.query_timeout_ms = 30_000;
    config.limits.parallel_scan_workers = 1;
    config.limits.parallel_aggregation_workers = 1;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let cassie = Arc::new(Cassie::new_in_memory_with_config(config.clone()).expect("Cassie"));
    cassie.startup().expect("start Boolean aggregate fixture");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("seed Boolean aggregate fixture");
    }
    drop(session);
    let server = start_server(Arc::clone(&cassie), config).await;
    let (mut reader, mut writer, process_id, secret_key) = connect(server.address).await;
    let sql =
        format!("SELECT BOOL_AND(flag) AS every_match, BOOL_OR(flag) AS any_match FROM {source}");
    wire::write_frames(&mut writer, vec![wire::simple_query_frame(&sql)]).await;
    let baseline = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    let dispatch = cassie.metrics()["parallel_aggregation"]["last_fallback_reason"]
        .as_str()
        .expect("aggregate dispatch reason")
        .to_owned();
    assert_empty_input_does_not_pause(&mut reader, &mut writer, source).await;

    let (_pause_guard, selected, resume) =
        cassie::runtime::pause_next_query_after_aggregate_input();
    wire::write_frames(&mut writer, vec![wire::simple_query_frame(&sql)]).await;
    tokio::task::spawn_blocking(move || selected.recv_timeout(Duration::from_secs(5)))
        .await
        .expect("Boolean input observer task")
        .expect("Boolean aggregate must retain input before cancellation");
    cancel_and_wait_for_close(server.address, process_id, secret_key).await;
    resume
        .send(())
        .expect("resume Boolean aggregate after cancellation was handled");
    let cancelled = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    let after_cancel = cassie.metrics();
    wire::write_frames(
        &mut writer,
        vec![wire::simple_query_frame("SELECT 46 AS recovery")],
    )
    .await;
    let recovery = wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;
    drop(reader);
    drop(writer);
    server.stop().await;
    CancellationCase {
        baseline,
        cancelled,
        recovery,
        dispatch,
        after_cancel,
    }
}

fn assert_case(case: &CancellationCase, expected_dispatch: &str) {
    assert_eq!(wire::error_code(&case.baseline), None);
    let description = case
        .baseline
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("baseline Boolean aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(&case.baseline),
        vec![vec![Some("f".to_owned()), Some("t".to_owned())]]
    );
    assert_eq!(case.dispatch, expected_dispatch);
    assert_eq!(wire::error_code(&case.cancelled).as_deref(), Some("57014"));
    assert!(wire::data_rows(&case.cancelled).is_empty());
    assert_eq!(
        case.cancelled
            .iter()
            .filter(|frame| frame.0 == b'C')
            .count(),
        0
    );
    assert_eq!(case.cancelled.last().map(|frame| frame.0), Some(b'Z'));
    assert_eq!(case.after_cancel["running_queries"].as_u64(), Some(0));
    assert_eq!(
        case.after_cancel["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        case.after_cancel["runtime"]["active_operator_workers"].as_u64(),
        Some(0)
    );
    assert_eq!(wire::error_code(&case.recovery), None);
    assert_eq!(
        wire::data_rows(&case.recovery),
        vec![vec![Some("46".to_owned())]]
    );
}

#[test]
fn should_cancel_boolean_aggregate_query_without_publishing_partial_output() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let case = runtime.block_on(run_case(&[], "(VALUES (TRUE), (FALSE)) AS inputs(flag)"));

    // Assert
    assert_case(&case, "worker-limit-one");
}

#[test]
fn should_cancel_native_boolean_aggregate_query_after_input_is_retained() {
    // Arrange
    let runtime = current_thread_runtime();
    let setup = [
        "CREATE TABLE issue_805_boolean_cancel (flag BOOLEAN)",
        "INSERT INTO issue_805_boolean_cancel VALUES (TRUE), (FALSE)",
    ];

    // Act
    let case = runtime.block_on(run_case(&setup, "issue_805_boolean_cancel"));

    // Assert
    assert_case(&case, "typed-worker-limit-one");
}
