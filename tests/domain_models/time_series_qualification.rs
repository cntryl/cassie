use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute fixture statement");
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
