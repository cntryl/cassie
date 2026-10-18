//! Boolean partial-state cases with empty and all-NULL worker inputs.

use super::*;

#[test]
fn should_merge_boolean_states_with_empty_null_partials() {
    // Arrange
    use_local_storage();
    let path = data_dir("postgresql18_boolean_aggregate_empty_partials");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie");
    let collection = "exec_pg18_boolean_aggregate_empty_partials";
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
    seed_empty_partial_documents(&cassie, collection);

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT bucket, \
                 BOOL_AND(CASE WHEN included THEN flag ELSE NULL END), \
                 BOOL_OR(CASE WHEN included THEN flag ELSE NULL END) \
             FROM exec_pg18_boolean_aggregate_empty_partials \
             GROUP BY bucket ORDER BY bucket",
            vec![],
        )
        .expect("parallel Boolean aggregates with empty partial inputs");
    let metrics = cassie.metrics();

    // Assert
    assert_eq!(
        result.rows(),
        vec![
            vec![
                Value::String("all_null".to_owned()),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::String("case_excluded".to_owned()),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::String("mixed".to_owned()),
                Value::Bool(false),
                Value::Bool(true),
            ],
            vec![
                Value::String("sparse".to_owned()),
                Value::Bool(true),
                Value::Bool(true),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 16, 16]
    );
    assert!(
        metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "Boolean states must use the partial merge path: {metrics}"
    );
    assert!(
        metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "expected Boolean partials from multiple workers: {metrics}"
    );
    assert_eq!(
        metrics["parallel_aggregation"]["rows"].as_u64(),
        Some(PARALLEL_ROW_COUNT as u64)
    );

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

fn seed_empty_partial_documents(cassie: &Cassie, collection: &str) {
    put_documents(
        cassie,
        collection,
        (0..PARALLEL_ROW_COUNT).map(|index| {
            let bucket = match index % 4 {
                0 => "mixed",
                1 => "all_null",
                2 => "case_excluded",
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
}
