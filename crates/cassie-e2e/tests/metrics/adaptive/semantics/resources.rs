#[test]
fn should_report_adaptive_candidate_budget_in_explain() {
    // Arrange
    let path = data_dir("adaptive_candidate_explain");
    let config = adaptive_candidate_config(2, 100);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_adaptive_candidate_explain";
        register_adaptive_candidate_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id, search_score(body, 'alpha') AS score FROM metrics_adaptive_candidate_explain ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = result.rows()[0][0].as_str().unwrap_or_default();
        assert!(
            plan.contains("candidate_budget=2"),
            "explain should include adaptive candidate budget: {plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
