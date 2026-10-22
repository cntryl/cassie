#[test]
fn should_prune_parameterized_time_series_ranges() {
    // Arrange
    let path = data_dir("time_series_parameterized_controls");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE ts_control_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .expect("table");
        for (tenant, event_at, amount) in [
            ("acme", "1969-12-31T23:00:00Z", 10_i64),
            ("acme", "1970-01-01T00:00:00Z", 20_i64),
            ("globex", "1970-01-01T01:00:00Z", 30_i64),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO ts_control_events (tenant, event_at, amount) VALUES ($1, $2, $3)",
                    vec![
                        Value::String(tenant.to_string()),
                        Value::String(event_at.to_string()),
                        Value::Int64(amount),
                    ],
                )
                .expect("insert");
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_control_events ON ts_control_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .expect("index");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM ts_control_events WHERE tenant = $1 AND event_at >= $2 AND event_at < $3 ORDER BY event_at",
                vec![
                    Value::String("acme".to_string()),
                    Value::String("1969-12-31T23:30:00Z".to_string()),
                    Value::String("1970-01-01T00:30:00Z".to_string()),
                ],
            )
            .expect("parameterized range");
        let cancellation = cassie::runtime::QueryCancellationHandle::new();
        cancellation.cancel();
        let cancelled = cassie.execute_sql_with_cancellation(
            &session,
            "SELECT amount FROM ts_control_events WHERE tenant = $1 AND event_at >= $2",
            vec![
                Value::String("acme".to_string()),
                Value::String("1969-12-31T00:00:00Z".to_string()),
            ],
            &cancellation,
        );
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows(), vec![vec![Value::Int64(20)]]);
        assert_eq!(metrics["time_series"]["bucket_native_hits"].as_u64(), Some(1));
        assert!(cancelled.expect_err("cancelled query").to_string().contains("canceled"));

        let _ = std::fs::remove_dir_all(path);
    });
}
