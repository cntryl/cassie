#[test]
fn should_record_adaptive_candidate_expansion() {
    // Arrange
    let path = data_dir("adaptive_candidate_expansion");
    let config = adaptive_candidate_config(1, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_adaptive_candidate_expansion";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_expansion ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(
            after["adaptive_candidates"]["decisions"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["decisions"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["initial_budget_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["initial_budget_total"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["final_candidate_count_total"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["final_candidate_count_total"]
                    .as_u64()
                    .unwrap_or_default(),
            3
        );
        assert!(
            after["adaptive_candidates"]["expansions_total"]
                .as_u64()
                .unwrap_or_default()
                > before["adaptive_candidates"]["expansions_total"]
                    .as_u64()
                    .unwrap_or_default(),
            "candidate work beyond the initial budget should be counted"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_the_selected_adaptive_read_surface_across_profiles() {
    // Arrange
    let evidence = AdaptiveProfileEvidence::collect();

    // Act
    evidence.write_requested_artifact();

    // Assert
    evidence.assert_equivalent();
}

#[test]
fn should_select_adaptive_read_operator_alternative() {
    // Arrange
    let path = data_dir("adaptive_read_operator_select");
    let config = adaptive_execution_config(true, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_adaptive_read_operator_select";
        let base_index = "metrics_adaptive_read_operator_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_select WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN ANALYZE SELECT title FROM metrics_adaptive_read_operator_select WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }
        let before = cassie.metrics();

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows()[0][0].as_str().unwrap().to_string();
        let after = cassie.metrics();

        // Assert
        assert!(plan.contains(preferred_index), "plan={plan}");
        assert!(plan.contains("adaptive_plan_enabled=true"), "plan={plan}");
        assert!(
            plan.contains(&format!(
                "adaptive_selected_alternative=index:{preferred_index}"
            )),
            "plan={plan}"
        );
        assert!(
            plan.contains("adaptive_reason=selected_operator_feedback"),
            "plan={plan}"
        );
        assert!(
            plan.contains("adaptive_plan_decisions_delta:1"),
            "plan={plan}"
        );
        assert!(
            plan.contains("adaptive_plan_selected_delta:1"),
            "plan={plan}"
        );
        assert_eq!(
            after["adaptive_candidates"]["plan_selected_alternatives"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["plan_selected_alternatives"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_plan_selected_alternative"],
            format!("index:{preferred_index}")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_adaptive_read_operator_disabled_without_selected_delta() {
    // Arrange
    let path = data_dir("adaptive_read_operator_disabled");
    let config = adaptive_execution_config(false, 0);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_adaptive_read_operator_disabled";
        let base_index = "metrics_adaptive_read_operator_disabled_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_disabled_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_disabled WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN ANALYZE SELECT title FROM metrics_adaptive_read_operator_disabled WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }
        let before = cassie.metrics();

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows()[0][0].as_str().unwrap().to_string();
        let after = cassie.metrics();

        // Assert
        assert!(plan.contains("operator_feedback=used"), "plan={plan}");
        assert!(
            plan.contains(&format!(
                "operator_feedback_selected_candidate=index:{preferred_index}"
            )),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_plan_enabled=false"), "plan={plan}");
        assert!(
            plan.contains(&format!("adaptive_base_alternative=index:{base_index}")),
            "plan={plan}"
        );
        assert!(
            plan.contains(&format!("adaptive_selected_alternative=index:{base_index}")),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_reason=disabled"), "plan={plan}");
        assert!(
            plan.contains("adaptive_plan_selected_delta:0"),
            "plan={plan}"
        );
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "plan_disabled_total"]
            ),
            1
        );
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "plan_selected_alternatives"]
            ),
            0
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_base_alternative_when_adaptive_guard_fails() {
    // Arrange
    let path = data_dir("adaptive_read_operator_guard");
    let config = adaptive_execution_config(true, 10_000);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_adaptive_read_operator_guard";
        let base_index = "metrics_adaptive_read_operator_guard_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_guard_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_guard WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN SELECT title FROM metrics_adaptive_read_operator_guard WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..4 {
            cassie
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows()[0][0].as_str().unwrap().to_string();

        // Assert
        assert!(plan.contains(base_index), "plan={plan}");
        assert!(
            plan.contains(&format!("adaptive_selected_alternative=index:{base_index}")),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_guard_passed=false"), "plan={plan}");
        assert!(plan.contains("adaptive_reason=guard_failed"), "plan={plan}");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_base_alternative_when_confidence_guard_fails() {
    // Arrange
    let path = data_dir("adaptive_read_operator_confidence");
    let config = adaptive_execution_confidence_config(true, 100, 900);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_adaptive_read_operator_confidence";
        let base_index = "metrics_adaptive_read_operator_confidence_body_idx_a";
        let preferred_index = "metrics_adaptive_read_operator_confidence_title_idx_b";
        register_feedback_collection(&cassie, collection);
        register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
        let session = cassie.create_session("tester", None);
        let shape_sql = "SELECT title FROM metrics_adaptive_read_operator_confidence WHERE title = 'alpha' AND body = 'one'";
        let explain_sql = "EXPLAIN SELECT title FROM metrics_adaptive_read_operator_confidence WHERE title = 'alpha' AND body = 'one'";
        let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
        let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));
        for _ in 0..3 {
            cassie
                .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                .expect("seed base feedback");
            cassie
                .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                .expect("seed preferred feedback");
        }

        // Act
        let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
        let plan = explain.rows()[0][0].as_str().unwrap().to_string();

        // Assert
        assert!(plan.contains(base_index), "plan={plan}");
        assert!(
            plan.contains(&format!("adaptive_selected_alternative=index:{base_index}")),
            "plan={plan}"
        );
        assert!(
            plan.contains("adaptive_guard=operator_feedback_confidence_bps:750>=900"),
            "plan={plan}"
        );
        assert!(plan.contains("adaptive_guard_passed=false"), "plan={plan}");
        assert!(
            plan.contains("adaptive_reason=confidence_guard_failed"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_switch_vectorized_join_to_merge_when_threshold_exceeded() {
    // Arrange
    let path = data_dir("operator_switch_join_threshold");
    let config = operator_switch_config(true, 2);
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
            "metrics_switch_users",
            "metrics_switch_orders",
        );
        let before = cassie.metrics();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_switch_users.name, metrics_switch_orders.total FROM metrics_switch_users JOIN metrics_switch_orders ON metrics_switch_users.user_key = metrics_switch_orders.order_user_key ORDER BY metrics_switch_users.name",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![
                vec![
                    cassie::types::Value::String("ada".to_string()),
                    cassie::types::Value::Int64(10)
                ],
                vec![
                    cassie::types::Value::String("grace".to_string()),
                    cassie::types::Value::Int64(20)
                ],
            ]
        );
        assert_eq!(
            after["adaptive_candidates"]["operator_switch_successes"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["operator_switch_successes"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_pair"],
            "vectorized_join_to_merge_join"
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_reason"],
            "row_threshold_exceeded"
        );
        assert_eq!(after["joins"]["last_strategy"], "merge");
        assert_eq!(after["joins"]["vectorized_joins"], 0);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_operator_switch_failure_without_claiming_success() {
    // Arrange
    let mut config = operator_switch_config(true, 0);
    // The 12 KiB boundary rejects query capture at 12,373 bytes before the
    // adaptive operator can be attempted; 13 KiB admits capture but rejects
    // the keyed replacement.
    config.limits.query_memory_budget_bytes = 13 * 1_024;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let session = cassie.create_session("tester", None);
        // Literal sources isolate replacement failure from storage-tree decode reservations.
        // Complete source rows fit; their independently owned keyed merge copies exceed
        // the shared budget, preserving failure at the replacement operator itself.
        let padding = "x".repeat(128);
        let left_sql = format!(
            "SELECT 1 AS user_key, 'ada' AS name, '{padding}' AS padding \
                 UNION ALL SELECT 2 AS user_key, 'grace' AS name, '{padding}' AS padding"
        );
        let right_sql = "SELECT 1 AS order_user_key, 10 AS total \
                             UNION ALL SELECT 2 AS order_user_key, 20 AS total";
        for input_sql in [left_sql.as_str(), right_sql] {
            let input = cassie
                .execute_sql(&session, input_sql, vec![])
                .expect("each complete replacement input must fit");
            assert_eq!(input.rows().len(), 2);
        }
        let sql = format!(
            "SELECT u.name, o.total \
             FROM ({left_sql}) AS u \
             JOIN ({right_sql}) AS o \
             ON u.user_key = o.order_user_key"
        );
        let before = cassie.metrics();

        // Act
        let result = cassie.execute_sql(&session, &sql, vec![]);
        let after = cassie.metrics();

        // Assert
        let error = result.expect_err("the replacement operator must exhaust the fixture budget");
        assert!(
            matches!(error, cassie::app::CassieError::ResourceLimit(_)),
            "replacement must fail at retained-state admission: {error:?}"
        );
        for path in [
            ["query", "rows_returned_total"],
            ["joins", "executions"],
            ["joins", "merge_joins"],
            ["joins", "vectorized_joins"],
            ["joins", "matched_rows_total"],
            ["joins", "output_rows_total"],
        ] {
            assert_eq!(snapshot_delta(&after, &before, &path), 0);
        }
        assert_eq!(after["query"]["current_accounted_memory_bytes"], 0);
        assert_eq!(after["runtime"]["running_queries"], 0);
        assert_eq!(after["runtime"]["active_operator_workers"], 0);
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "operator_switch_attempts"]
            ),
            1,
            "operator replacement should be attempted before failing: {error:?}"
        );
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "operator_switch_successes"]
            ),
            0
        );
        assert_eq!(
            snapshot_delta(
                &after,
                &before,
                &["adaptive_candidates", "operator_switch_fallbacks"]
            ),
            1
        );
        assert_eq!(
            after["adaptive_candidates"]["last_operator_switch_reason"],
            "replacement_failed"
        );

        drop(session);
        drop(cassie);
    });
}

#[test]
fn should_keep_vectorized_join_when_operator_switching_disabled() {
    // Arrange
    let path = data_dir("operator_switch_disabled");
    let config = operator_switch_config(false, 0);
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
            "metrics_switch_disabled_users",
            "metrics_switch_disabled_orders",
        );
        let before = cassie.metrics();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT metrics_switch_disabled_users.name, metrics_switch_disabled_orders.total FROM metrics_switch_disabled_users JOIN metrics_switch_disabled_orders ON metrics_switch_disabled_users.user_key = metrics_switch_disabled_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(selected.rows().len(), 2);
        assert_eq!(
            after["adaptive_candidates"]["operator_switch_attempts"]
                .as_u64()
                .unwrap_or_default()
                - before["adaptive_candidates"]["operator_switch_attempts"]
                    .as_u64()
                    .unwrap_or_default(),
            0
        );
        assert_eq!(after["joins"]["last_strategy"], "vectorized");
        assert_eq!(after["joins"]["vectorized_joins"], 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_runtime_operator_switch_in_explain_analyze() {
    // Arrange
    let path = data_dir("operator_switch_explain");
    let config = operator_switch_config(true, 1);
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
            "metrics_switch_explain_users",
            "metrics_switch_explain_orders",
        );

        // Act
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN ANALYZE SELECT metrics_switch_explain_users.name, metrics_switch_explain_orders.total FROM metrics_switch_explain_users JOIN metrics_switch_explain_orders ON metrics_switch_explain_users.user_key = metrics_switch_explain_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let plan = explained.rows()[0][0].as_str().unwrap_or_default();

        // Assert
        assert!(plan.contains("operator_switch_enabled=true"), "plan={plan}");
        assert!(
            plan.contains("operator_switch_pair=vectorized_join_to_merge_join"),
            "plan={plan}"
        );
        assert!(plan.contains("operator_switch_reason=armed"), "plan={plan}");
        assert!(
            plan.contains("operator_switch_success_delta:1"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
