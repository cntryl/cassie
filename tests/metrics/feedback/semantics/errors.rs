#[test]
fn should_reject_feedback_record_from_stale_schema_epoch() {
    // Arrange
    use_local_storage();
    let path = data_dir("feedback_stale_schema_epoch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let collection = "metrics_feedback_stale_schema_epoch";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_stale_schema_epoch";
        let stale_key = feedback_key(&cassie, &session, sql, None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE feedback_new_schema_epoch (id INT)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .seed_feedback_for_diagnostics(&stale_key, &confident_feedback(5, 1))
            .expect("discard stale-epoch feedback");
        let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

        // Assert
        assert!(cassie.feedback_record_for_diagnostics(&stale_key).is_none());
        assert!(
            persisted
                .iter()
                .all(|(persisted_key, _)| persisted_key != &stale_key),
            "persisted={persisted:?}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
