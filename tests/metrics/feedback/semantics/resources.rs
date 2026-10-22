#[test]
fn should_evict_runtime_feedback_when_retention_limit_is_exceeded() {
    // Arrange
    let path = data_dir("feedback_eviction");
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.feedback_entries = 1;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_feedback_eviction";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let first_sql = "SELECT title FROM metrics_feedback_eviction";
        let second_sql = "SELECT body FROM metrics_feedback_eviction";
        let first_key = feedback_key(&cassie, &session, first_sql, None);

        // Act
        cassie.execute_sql(&session, first_sql, vec![]).unwrap();
        cassie.execute_sql(&session, second_sql, vec![]).unwrap();
        let metrics = cassie.metrics();
        let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

        // Assert
        assert!(cassie.feedback_record_for_diagnostics(&first_key).is_none());
        assert_eq!(metrics["feedback"]["entries"].as_u64(), Some(1));
        assert_eq!(persisted.len(), 1, "persisted={persisted:?}");
        assert_ne!(persisted[0].0, first_key);
        assert!(
            metrics["feedback"]["evictions"]
                .as_u64()
                .unwrap_or_default()
                >= 1,
            "retention limit should evict the oldest feedback"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_runtime_feedback_for_candidate_budget() {
    // Arrange
    let path = data_dir("adaptive_candidate_feedback");
    let config = adaptive_candidate_config(1, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_in_memory_with_config(config).unwrap();
            let collection = "metrics_adaptive_candidate_feedback";
            register_adaptive_candidate_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_feedback ORDER BY score DESC LIMIT 1";
            let wider_sql = "SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_feedback ORDER BY score DESC LIMIT 2";

            cassie.execute_sql(&session, sql, vec![]).unwrap();
            let seeded = cassie.metrics();

            // Act
            cassie.execute_sql(&session, wider_sql, vec![]).unwrap();
            let after = cassie.metrics();

            // Assert
            assert_eq!(
                after["adaptive_candidates"]["initial_budget_total"]
                    .as_u64()
                    .unwrap_or_default()
                    - seeded["adaptive_candidates"]["initial_budget_total"]
                        .as_u64()
                        .unwrap_or_default(),
                3
            );
            assert_eq!(
                after["adaptive_candidates"]["feedback_budget_total"]
                    .as_u64()
                    .unwrap_or_default()
                    - seeded["adaptive_candidates"]["feedback_budget_total"]
                        .as_u64()
                        .unwrap_or_default(),
                3
            );

            let _ = std::fs::remove_dir_all(path);
        });
}
