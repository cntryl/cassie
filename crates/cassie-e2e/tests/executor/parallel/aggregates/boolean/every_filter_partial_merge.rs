//! EVERY alias behavior with FILTER across Boolean partial-state merges.

use super::*;

#[test]
fn should_merge_filtered_every_states_across_workers() {
    // Arrange
    let path = data_dir("postgresql18_every_filter_partial_merge");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
    let collection = "exec_pg18_every_filter_partial_merge";
    let session = cassie.create_session("tester", None);
    create_registered_collection(
        &cassie,
        collection,
        &[
            ("bucket", DataType::Text),
            ("flag", DataType::Boolean),
            ("included", DataType::Boolean),
        ],
    );
    put_documents(
        &cassie,
        collection,
        (0..PARALLEL_ROW_COUNT).map(|index| {
            let bucket = match index % 4 {
                0 => "mixed",
                1 => "all_null",
                2 => "empty_filter",
                _ => "sparse",
            };
            let flag = if index % 4 == 1 {
                serde_json::Value::Null
            } else if index == 0 {
                serde_json::Value::Bool(false)
            } else {
                serde_json::Value::Bool(true)
            };
            let included = match index % 4 {
                2 => false,
                3 => index < 128,
                _ => true,
            };
            (
                format!("doc-{index:04}"),
                serde_json::json!({
                    "bucket": bucket,
                    "flag": flag,
                    "included": included,
                }),
            )
        }),
    );

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT bucket, EVERY(flag) FILTER (WHERE included) \
             FROM exec_pg18_every_filter_partial_merge \
             GROUP BY bucket ORDER BY bucket",
            vec![],
        )
        .expect("parallel filtered EVERY aggregate query");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(
        result.rows(),
        vec![
            vec![Value::String("all_null".to_owned()), Value::Null],
            vec![Value::String("empty_filter".to_owned()), Value::Null],
            vec![Value::String("mixed".to_owned()), Value::Bool(false)],
            vec![Value::String("sparse".to_owned()), Value::Bool(true)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 16]
    );
    assert!(
        metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "filtered EVERY must use the partial-state merge path: {metrics}"
    );
    assert!(
        metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "expected EVERY partials from multiple workers: {metrics}"
    );
    assert_eq!(
        metrics["parallel_aggregation"]["rows"].as_u64(),
        Some(PARALLEL_ROW_COUNT as u64)
    );
    assert_eq!(metrics["parallel_aggregation"]["groups"].as_u64(), Some(4));

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
