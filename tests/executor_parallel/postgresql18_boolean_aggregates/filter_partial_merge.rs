//! Aggregate FILTER semantics across Boolean partial-state merges.

use super::*;

#[test]
fn should_apply_boolean_aggregate_filters_before_partial_merge() {
    // Arrange
    use_local_storage();
    let path = data_dir("postgresql18_boolean_filter_partial_merge");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie");
    let collection = "exec_pg18_boolean_filter_partial_merge";
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
            } else if index == 0 || index == 515 {
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
            "SELECT bucket, \
                 BOOL_AND(flag) FILTER (WHERE included), \
                 BOOL_OR(flag) FILTER (WHERE included) \
             FROM exec_pg18_boolean_filter_partial_merge \
             GROUP BY bucket ORDER BY bucket",
            vec![],
        )
        .expect("parallel Boolean aggregate FILTER query");
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
                Value::String("empty_filter".to_owned()),
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
        "filtered Boolean states must use the partial merge path: {metrics}"
    );
    assert!(
        metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "expected filtered Boolean partials from multiple workers: {metrics}"
    );
    assert_eq!(
        metrics["parallel_aggregation"]["rows"].as_u64(),
        Some(PARALLEL_ROW_COUNT as u64)
    );
    assert_eq!(metrics["parallel_aggregation"]["groups"].as_u64(), Some(4));

    // Count OIDs are checked separately, after the Boolean-only metrics snapshot.
    let count_result = cassie
        .execute_sql(
            &session,
            "SELECT bucket, \
                 COUNT(flag) FILTER (WHERE included), \
                 COUNT(*) FILTER (WHERE included) \
             FROM exec_pg18_boolean_filter_partial_merge \
             GROUP BY bucket ORDER BY bucket",
            vec![],
        )
        .expect("filtered count metadata query");
    assert_eq!(
        count_result.rows(),
        vec![
            vec![
                Value::String("all_null".to_owned()),
                Value::Int64(0),
                Value::Int64(256),
            ],
            vec![
                Value::String("empty_filter".to_owned()),
                Value::Int64(0),
                Value::Int64(0),
            ],
            vec![
                Value::String("mixed".to_owned()),
                Value::Int64(257),
                Value::Int64(257),
            ],
            vec![
                Value::String("sparse".to_owned()),
                Value::Int64(32),
                Value::Int64(32),
            ],
        ]
    );
    assert_eq!(
        count_result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 20, 20]
    );

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
