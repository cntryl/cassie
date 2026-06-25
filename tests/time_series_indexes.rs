#![allow(unused_imports, dead_code)]

use cassie::app::Cassie;
use cassie::catalog::IndexKind;
use cassie::sql::ast::QueryStatement;
use cassie::types::Value;
use serde_json::json;

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
        assert!(plan.contains("time_series_storage=bucket-native-v1"));
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
        let sidecars =
            time_series_sidecar_records(&cassie, "ts_execute_events", "idx_ts_execute_time");

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
        assert_eq!(sidecars.len(), 3);
        assert_eq!(metrics["time_series"]["scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
        );
        assert_eq!(metrics["time_series"]["rows"].as_u64(), Some(2));
        assert_eq!(metrics["time_series"]["buckets_scanned"].as_u64(), Some(2));
        assert_eq!(metrics["time_series"]["buckets_skipped"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["last_index"].as_str(),
            Some("idx_ts_execute_time")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_bulk_load_fresh_time_series_documents_for_bucket_reads() {
    // Arrange
    with_fallback();
    let path = data_dir("time_series_fresh_bulk_load");
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
                "CREATE TABLE ts_fresh_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_fresh_time ON ts_fresh_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        cassie
            .midge
            .put_fresh_time_series_documents(
                "ts_fresh_events",
                vec![
                    (
                        Some("event-1".to_string()),
                        json!({
                            "tenant": "acme",
                            "event_at": "2026-01-01T00:00:00Z",
                            "amount": 10,
                        }),
                    ),
                    (
                        Some("event-2".to_string()),
                        json!({
                            "tenant": "acme",
                            "event_at": "2026-01-01T01:00:00Z",
                            "amount": 20,
                        }),
                    ),
                    (
                        Some("event-3".to_string()),
                        json!({
                            "tenant": "globex",
                            "event_at": "2026-01-01T01:00:00Z",
                            "amount": 30,
                        }),
                    ),
                ],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM ts_fresh_events WHERE event_at >= '2026-01-01T01:00:00Z' ORDER BY tenant, amount",
                vec![],
            )
            .unwrap();
        let sidecars = time_series_sidecar_records(
            &cassie,
            "ts_fresh_events",
            "idx_ts_fresh_time",
        );
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows,
            vec![
                vec![Value::String("acme".to_string()), Value::Int64(20)],
                vec![Value::String("globex".to_string()), Value::Int64(30)],
            ]
        );
        assert_eq!(sidecars.len(), 3);
        assert_eq!(metrics["time_series"]["bucket_native_hits"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_time_series_range_reads_after_mutations_restart() {
    // Arrange
    with_fallback();
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

        // Assert
        assert_eq!(result.rows, vec![vec![cassie::types::Value::Int64(20)]]);
        let plan = match &explain.rows[0][0] {
            cassie::types::Value::String(value) => value,
            other => panic!("expected explain string, got {other:?}"),
        };
        assert!(plan.contains("index=idx_ts_mutation_time"));
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
            Some("idx_ts_mutation_time")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fallback_to_row_blobs_when_bucket_membership_is_missing() {
    // Arrange
    with_fallback();
    let path = data_dir("time_series_index_missing_sidecar");
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
                "CREATE TABLE ts_missing_bucket_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_missing_bucket_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-01T01:00:00Z', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_missing_bucket_time ON ts_missing_bucket_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        clear_time_series_sidecars(
            &cassie,
            "ts_missing_bucket_events",
            "idx_ts_missing_bucket_time",
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM ts_missing_bucket_events WHERE event_at >= '2026-01-01T01:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows, vec![vec![Value::Int64(20)]]);
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(0)
        );
        assert_eq!(metrics["time_series"]["fallback_scans"].as_u64(), Some(1));
        assert_eq!(
            metrics["time_series"]["last_fallback_reason"].as_str(),
            Some("missing-bucket-metadata")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cleanup_bucket_membership_after_retention() {
    // Arrange
    with_fallback();
    let path = data_dir("time_series_index_retention_sidecar");
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
                "CREATE TABLE ts_retention_bucket_events (tenant TEXT, event_at TIMESTAMP, amount INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO ts_retention_bucket_events (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:00:00Z', 10), ('acme', '2026-01-03T00:00:00Z', 20)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX idx_ts_retention_bucket_time ON ts_retention_bucket_events USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
                vec![],
            )
            .unwrap();
        let sidecars_before = time_series_sidecar_records(
            &cassie,
            "ts_retention_bucket_events",
            "idx_ts_retention_bucket_time",
        );
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY ts_retention_bucket_policy ON ts_retention_bucket_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY ts_retention_bucket_policy AT '2026-01-03T12:00:00Z'",
                vec![],
            )
            .unwrap();
        let sidecars_after = time_series_sidecar_records(
            &cassie,
            "ts_retention_bucket_events",
            "idx_ts_retention_bucket_time",
        );
        let result = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM ts_retention_bucket_events WHERE event_at >= '2026-01-01T00:00:00Z' ORDER BY event_at",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(sidecars_before.len(), 2);
        assert_eq!(sidecars_after.len(), 1);
        assert_eq!(result.rows, vec![vec![Value::Int64(20)]]);
        assert_eq!(
            metrics["time_series"]["bucket_native_hits"].as_u64(),
            Some(1)
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
