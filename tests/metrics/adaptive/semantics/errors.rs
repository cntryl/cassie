#[test]
fn should_reject_adaptive_candidate_cap_overflow() {
    // Arrange
    let path = data_dir("adaptive_candidate_cap");
    let config = adaptive_candidate_config(1, 1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_adaptive_candidate_cap";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_cap ORDER BY score DESC LIMIT 2",
                vec![],
            )
            .expect_err("query should exceed adaptive candidate cap");
        let after = cassie.metrics();

        // Assert
        assert!(
            error
                .to_string()
                .contains("adaptive candidate max"),
            "error should name the adaptive candidate cap: {error}"
        );
        assert_eq!(
            after["adaptive_candidates"]["limit_errors_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["limit_errors_total"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_skip_runtime_operator_switch_for_unsupported_join_type() {
    // Arrange
    let path = data_dir("operator_switch_unsupported");
    let config = operator_switch_config(true, 0);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let session = cassie.create_session("tester", None);
        create_switch_join_tables(
            &cassie,
            &session,
            "metrics_switch_right_users",
            "metrics_switch_right_orders",
        );
        let before = cassie.metrics();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_switch_right_users.name, metrics_switch_right_orders.total FROM metrics_switch_right_users RIGHT JOIN metrics_switch_right_orders ON metrics_switch_right_users.user_key = metrics_switch_right_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(selected.rows().len(), 2);
        assert_eq!(
            after["adaptive_candidates"]["operator_switch_skips"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["operator_switch_skips"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_reason"],
            "unsupported_join_type"
        );
        assert_eq!(after["joins"]["last_strategy"], "merge");

        let _ = std::fs::remove_dir_all(path);
    });
}
