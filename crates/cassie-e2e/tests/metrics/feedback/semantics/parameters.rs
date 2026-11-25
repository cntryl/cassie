#[test]
fn should_aggregate_runtime_feedback_across_parameter_values() {
    // Arrange
    let path = data_dir("feedback_aggregate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let collection = "metrics_feedback_aggregate";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_aggregate WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, None);

        // Act
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .unwrap();
        let metrics = cassie.metrics();
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("scan feedback should aggregate");

        // Assert
        assert_eq!(record.executions, 2);
        assert_eq!(record.rows_out_total, 2);
        assert!(
            metrics["feedback"]["hits"].as_u64().unwrap_or_default() >= 1,
            "second feedback lookup should hit"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
