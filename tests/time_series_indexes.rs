#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::IndexKind;
use cassie::sql::ast::QueryStatement;

#[path = "support/sql.rs"]
mod support;
use support::*;

#[test]
fn should_parse_time_series_index_options() {
    // Arrange
    let sql = "CREATE INDEX idx_events_time ON events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = 'tenant,status')";

    // Act
    let parsed = cassie::sql::parse_statement(sql).unwrap();

    // Assert
    let QueryStatement::CreateIndex(statement) = parsed.statement else {
        panic!("expected CREATE INDEX");
    };
    assert_eq!(statement.kind, IndexKind::TimeSeries);
    assert_eq!(statement.fields, vec!["event_at"]);
    assert_eq!(
        statement.options.get("bucket_width"),
        Some(&"1 hour".to_string())
    );
    assert_eq!(
        statement.options.get("partition_by"),
        Some(&"tenant,status".to_string())
    );
}

#[test]
fn should_select_time_series_index_for_timestamp_range_explain() {
    // Arrange
    with_fallback();
    let path = data_dir("time_series_index_explain");
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
                "CREATE TABLE ts_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_events_time ON ts_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();

        // Act
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT tenant FROM ts_events WHERE event_at >= '2026-01-01T00:00:00Z'",
                vec![],
            )
            .unwrap();

        // Assert
        let plan = match &explained.rows[0][0] {
            cassie::types::Value::String(value) => value,
            other => panic!("expected explain string, got {other:?}"),
        };
        assert!(plan.contains("index=idx_ts_events_time"));
        assert!(plan.contains("time_series=bucket_width:1 hour"));
        assert!(plan.contains("partition_by:tenant"));
        assert!(plan.contains("range_filter:true"));
        assert!(plan.contains("cost_model=v2"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_execute_timestamp_range_with_time_series_metrics() {
    // Arrange
    with_fallback();
    let path = data_dir("time_series_index_execute");
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
                "CREATE TABLE ts_execute_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_execute_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-01T01:00:00Z', 20), ('acme', '2026-01-02T00:00:00Z', 30)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_execute_time ON ts_execute_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM ts_execute_events WHERE event_at >= '2026-01-01T01:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![
                    cassie::types::Value::String("acme".to_string()),
                    cassie::types::Value::Int64(20),
                ],
                vec![
                    cassie::types::Value::String("acme".to_string()),
                    cassie::types::Value::Int64(30),
                ],
            ]
        );
        assert_eq!(metrics["time_series"]["scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["last_index"].as_str(),
            Some("idx_ts_execute_time")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_time_series_index_on_non_timestamp_field() {
    // Arrange
    with_fallback();
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
