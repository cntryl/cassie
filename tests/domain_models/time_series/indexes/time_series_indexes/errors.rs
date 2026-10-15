#[test]
fn should_reject_unsupported_time_series_bucket_width() {
    // Arrange
    use_local_storage();
    let path = data_dir("time_series_unsupported_bucket_width");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE ts_unsupported_width_events (event_at TIMESTAMP)",
            vec![],
        )
        .unwrap();

    // Act
    let errors = [
            "0 minutes",
            "-1 hour",
            "1 second",
            "1 week",
            "1 month",
            "1 calendar day",
            "18446744073709551615 days",
        ]
        .map(|width| {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "CREATE INDEX idx_ts_unsupported_width ON ts_unsupported_width_events USING time_series (event_at) WITH (bucket_width = '{width}')"
                    ),
                    vec![],
                )
                .expect_err("unsupported width must not create a row-backed time-series index")
        });

    // Assert
    for error in errors {
        assert!(
            error
                .to_string()
                .contains("bucket_width must be a positive minute, hour, or day interval"),
            "error={error}"
        );
    }

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_time_series_index_on_non_timestamp_field() {
    // Arrange
    use_local_storage();
    let path = data_dir("time_series_index_invalid_type");
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
                "CREATE TABLE ts_bad_events (event_at TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_bad_events_time ON ts_bad_events USING time_series (event_at)",
                vec![],
            )
            .unwrap_err();

        // Assert
        assert!(error.to_string().contains("requires timestamp field"));

        let _ = std::fs::remove_dir_all(path);
    });
}
