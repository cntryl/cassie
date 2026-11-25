//! Finite encoded projection fixture shared by resource-demand witnesses.
use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

pub fn fixture() -> (Cassie, CassieSession, String) {
    let mut config = CassieRuntimeConfig::from_env().expect("configuration");
    config.limits.query_memory_budget_bytes = 1024 * 1024;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_in_memory_with_config(config).expect("engine");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE records (payload TEXT)", vec![])
        .expect("table");
    let payload = "x".repeat(8192);
    let documents = (0..8)
        .map(|index| {
            (
                Some(format!("record-{index:02}")),
                serde_json::json!({"payload": payload}),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents("records", documents)
        .expect("records");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX records_payload ON records USING column (payload) WITH (segment_size = 1)",
            vec![],
        )
        .expect("column index");

    (cassie, session, payload)
}
