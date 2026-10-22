#[test]
fn should_not_cache_transaction_control_statements() {
    // Arrange
    let path = data_dir("transaction_controls_bypass_cache");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("alice", None);
        let before = cassie.metrics();

        // Act
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie.execute_sql(&session, "COMMIT", vec![]).unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            after["plan_cache"]["hits"].as_u64(),
            before["plan_cache"]["hits"].as_u64()
        );
        assert_eq!(
            after["plan_cache"]["misses"].as_u64(),
            before["plan_cache"]["misses"].as_u64()
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
