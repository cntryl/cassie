#[test]
fn should_drop_metrics_fixture_without_shutdown_timeout() {
    // Arrange
    let owning_fixture = should_report_operator_switch_failure_without_claiming_success;

    // Act
    let captured = super::support_teardown_evidence::capture_warnings(owning_fixture);

    // Assert
    assert!(
        !captured.contains("Midge graceful shutdown did not complete"),
        "fixture must drop engine owners before removing fencing state: {captured}"
    );
}

#[test]
fn should_cancel_before_operator_switch_without_leaking_resources_or_success() {
    // Arrange
    use_local_storage();
    let path = data_dir("operator_switch_cancellation");
    let mut config = operator_switch_config(true, 0);
    config.limits.max_query_workers = 1;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            let session = cassie.create_session("tester", None);
            create_switch_join_tables(
                &cassie,
                &session,
                "metrics_switch_cancel_users",
                "metrics_switch_cancel_orders",
            );
            let cancellation = QueryCancellationHandle::new();
            cancellation.cancel();
            let before = cassie.metrics();

            // Act
            let result = cassie.execute_sql_with_cancellation(
                &session,
                "SELECT metrics_switch_cancel_users.name, metrics_switch_cancel_orders.total FROM metrics_switch_cancel_users JOIN metrics_switch_cancel_orders ON metrics_switch_cancel_users.user_key = metrics_switch_cancel_orders.order_user_key",
                vec![],
                &cancellation,
            );
            let after = cassie.metrics();

            // Assert
            assert!(matches!(result, Err(cassie::CassieError::QueryCancelled)));
            assert_eq!(
                snapshot_delta(
                    &after,
                    &before,
                    &["adaptive_candidates", "operator_switch_successes"]
                ),
                0
            );
            assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
            assert_eq!(after["runtime"]["active_operator_workers"], 0);

            let _ = std::fs::remove_dir_all(path);
        });
}
