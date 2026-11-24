#[test]
fn should_preserve_time_series_range_reads_after_mutations_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("time_series_index_mutations");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_mutation_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_mutation_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-01T01:00:00Z', 20), ('acme', '2026-01-01T03:00:00Z', 30)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_mutation_time ON ts_mutation_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE ts_mutation_events SET event_at = '2026-01-01T04:00:00Z' WHERE amount = 20",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM ts_mutation_events WHERE amount = 30",
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);

        // Act
        let result = restarted
            .execute_sql(
                &restarted_session,
                "SELECT amount FROM ts_mutation_events WHERE event_at >= '2026-01-01T02:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let explain = restarted
            .execute_sql(
                &restarted_session,
                "EXPLAIN SELECT amount FROM ts_mutation_events WHERE event_at >= '2026-01-01T02:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = restarted.metrics();
        let sidecars = time_series_sidecar_records(
            &restarted,
            "ts_mutation_events",
            "idx_ts_mutation_time",
        );
        let index_name =
            canonical_test_index(&restarted, "ts_mutation_events", "idx_ts_mutation_time");

        // Assert
        assert_eq!(result.rows(), vec![vec![cassie::types::Value::Int64(20)]]);
        let plan = match &explain.rows()[0][0] {
            cassie::types::Value::String(value) => value,
            other => panic!("expected explain string, got {other:?}"),
        };
        assert!(plan.contains(&format!(
            "index={}",
            canonical_test_index(&restarted, "ts_mutation_events", "idx_ts_mutation_time")
        )));
        assert!(plan.contains("time_series=bucket_width:1 hour"));
        assert!(plan.contains("time_series_storage=bucket-native-v1"));
        assert_eq!(sidecars.len(), 2);
        assert_eq!(metrics["time_series"]["scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
        );
        assert_eq!(
            metrics["time_series"]["last_index"].as_str(),
            Some(index_name.as_str())
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
