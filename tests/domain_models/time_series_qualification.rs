use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute fixture statement");
}

fn assert_fixed_utc_day_spacing() {
    let first_day = time::OffsetDateTime::parse(
        "2024-03-10T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .expect("first UTC day");
    let second_day = time::OffsetDateTime::parse(
        "2024-03-11T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .expect("second UTC day");
    assert_eq!(second_day - first_day, time::Duration::seconds(86_400));
}

fn assert_native_time_series_path(before: &serde_json::Value, after: &serde_json::Value) {
    let counter = |metrics: &serde_json::Value, field: &str| {
        metrics["time_series"][field].as_u64().unwrap_or_default()
    };
    assert_eq!(
        counter(after, "bucket_native_hits") - counter(before, "bucket_native_hits"),
        1
    );
    assert_eq!(
        counter(after, "fallback_scans") - counter(before, "fallback_scans"),
        0
    );
    assert_eq!(
        after["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_preserve_signed_zero_float_partition_rows() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    super::support_sql::use_local_storage();
    let path = super::support_sql::data_dir("signed-zero-qualification");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("qualification", None);
        for table in ["zero_baseline", "zero_indexed"] {
            execute(&cassie, &session, &format!("CREATE TABLE {table} (tenant FLOAT, event_at TIMESTAMP, amount INT)"));
            execute(&cassie, &session, &format!("INSERT INTO {table} (tenant, event_at, amount) VALUES (-0.0, '1969-12-31T23:59:59.75Z', 10), (0.0, '1970-01-01T00:00:00Z', 20), (1.0, '1970-01-01T00:00:00Z', 30)"));
        }
        execute(&cassie, &session, "CREATE INDEX zero_time ON zero_indexed USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)");
        for table in ["zero_baseline", "zero_indexed"] {
            let collection = cassie.catalog.get_schema(table).expect("table").collection;
            cassie.midge.put_fresh_time_series_documents(&collection, vec![
                (Some("stored-negative-zero".to_owned()), serde_json::json!({"tenant":-0.0,"event_at":"1970-01-01T00:00:00.25Z","amount":40})),
                (Some("stored-positive-zero".to_owned()), serde_json::json!({"tenant":0.0,"event_at":"1970-01-01T00:00:00.5Z","amount":50})),
            ]).expect("retain both stored signed-zero spellings");
        }
        let before = cassie.metrics();
        // Act
        let results = [0.0, -0.0, 1.0].map(|partition| {
            let rows = |table: &str| cassie.execute_sql(&session, &format!("SELECT amount FROM {table} WHERE tenant = $1 AND event_at >= '1969-12-31T23:59:59Z' AND event_at < '1970-01-01T00:00:01Z' ORDER BY amount"), vec![Value::Float64(partition)]).expect("range query").rows;
            (rows("zero_baseline"), rows("zero_indexed"))
        });
        let after = cassie.metrics();
        // Assert
        for (baseline, indexed) in &results { assert_eq!(indexed, baseline); }
        assert_eq!(results[0].0, vec![vec![Value::Int64(10)], vec![Value::Int64(20)], vec![Value::Int64(40)], vec![Value::Int64(50)]]);
        assert_eq!(results[1].0, results[0].0);
        assert_eq!(results[2].0, vec![vec![Value::Int64(30)]]);
        let counter = |metrics: &serde_json::Value, field: &str| metrics["time_series"][field].as_u64().unwrap_or_default();
        assert_eq!(counter(&after,"bucket_native_hits") - counter(&before,"bucket_native_hits"),3);
        assert_eq!(counter(&after,"fallback_scans") - counter(&before,"fallback_scans"),0);
        assert_eq!(after["query"]["current_accounted_memory_bytes"].as_u64(),Some(0));
        drop(cassie);
        std::fs::remove_dir_all(path).expect("remove fixture directory");
    });
}

#[test]
fn should_use_fixed_utc_day_buckets_across_spring_dst_transition() {
    // Arrange
    let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
    super::support_sql::use_local_storage();
    let path = super::support_sql::data_dir("time-series-spring-dst-fixed-day");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("config");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("qualification", None);
        for table in ["dst_day_baseline", "dst_day_indexed"] {
            execute(
                &cassie,
                &session,
                &format!("CREATE TABLE {table} (tenant TEXT, event_at TIMESTAMP, amount INT)"),
            );
            execute(
                &cassie,
                &session,
                &format!(
                    "INSERT INTO {table} (tenant,event_at,amount) VALUES \
                     ('a','2024-03-10T00:30:00-05:00',1), \
                     ('a','2024-03-10T23:30:00-04:00',2)"
                ),
            );
        }
        execute(
            &cassie,
            &session,
            "CREATE INDEX dst_day_time ON dst_day_indexed USING time_series \
             (event_at) WITH (bucket_width = '1 day', partition_by = tenant)",
        );
        let query = |table: &str| {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "SELECT event_at, amount \
                         FROM {table} WHERE tenant='a' \
                         AND event_at >= '2024-03-10T00:00:00Z' \
                         AND event_at < '2024-03-12T00:00:00Z' ORDER BY event_at"
                    ),
                    vec![],
                )
                .expect("query DST boundary rows")
                .rows
        };
        let baseline = query("dst_day_baseline");
        let buckets = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 day',event_at) AS utc_day \
                 FROM dst_day_baseline ORDER BY event_at",
                vec![],
            )
            .expect("bucket timestamps by fixed UTC days")
            .rows;
        let before = cassie.metrics();

        // Act
        let indexed = query("dst_day_indexed");
        let after = cassie.metrics();

        // Assert
        assert_fixed_utc_day_spacing();
        assert_eq!(indexed, baseline);
        assert_eq!(
            indexed,
            vec![
                vec![
                    Value::String("2024-03-10T05:30:00.000000Z".into()),
                    Value::Int64(1),
                ],
                vec![
                    Value::String("2024-03-11T03:30:00.000000Z".into()),
                    Value::Int64(2),
                ],
            ]
        );
        assert_eq!(
            buckets,
            vec![
                vec![Value::String("2024-03-10T00:00:00Z".into())],
                vec![Value::String("2024-03-11T00:00:00Z".into())],
            ]
        );
        assert_native_time_series_path(&before, &after);
        drop(cassie);
        std::fs::remove_dir_all(path).expect("remove fixture directory");
    });
}
