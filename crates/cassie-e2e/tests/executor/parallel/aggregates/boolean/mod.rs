//! PostgreSQL 18 Boolean aggregate partial-state merge contract.

use super::*;

mod empty_partial_inputs;
mod every_filter_partial_merge;
mod filter_partial_merge;

#[test]
fn should_merge_boolean_aggregate_states_across_workers() {
    // Arrange
    let path = data_dir("postgresql18_boolean_aggregate_merge");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_pg18_boolean_aggregate_merge";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("flag", DataType::Boolean)]);
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let flag = match index {
                    0 => serde_json::Value::Bool(false),
                    512 => serde_json::Value::Null,
                    _ => serde_json::Value::Bool(true),
                };
                (format!("doc-{index:04}"), serde_json::json!({"flag": flag}))
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT BOOL_AND(flag), BOOL_OR(flag) FROM exec_pg18_boolean_aggregate_merge",
                vec![],
            )
            .expect("parallel Boolean aggregate query");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![Value::Bool(false), Value::Bool(true)]]
        );
        assert!(
            metrics["parallel_aggregation"]["aggregations"]
                .as_u64()
                .unwrap_or(0)
                > 0,
            "Boolean aggregate should use the partial-state merge path: {metrics}"
        );
        assert!(
            metrics["parallel_aggregation"]["workers"]
                .as_u64()
                .unwrap_or(0)
                >= 2,
            "expected multiple aggregate workers: {metrics}"
        );
        assert_eq!(
            metrics["parallel_aggregation"]["rows"].as_u64(),
            Some(PARALLEL_ROW_COUNT as u64)
        );
    });

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
