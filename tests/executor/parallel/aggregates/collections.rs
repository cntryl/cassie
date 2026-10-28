//! PostgreSQL 18 aggregate and JSON collection behavior across executor paths.

use super::*;

#[test]
fn should_execute_grouped_boolean_aggregates_with_partial_states() {
    // Arrange
    let path = data_dir("postgresql18_grouped_boolean_aggregate_states");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_in_memory_with_config(config).expect("Cassie");
    let collection = "exec_pg18_grouped_boolean_aggregate_states";
    let session = cassie.create_session("tester", None);
    create_registered_collection(
        &cassie,
        collection,
        &[("bucket", DataType::Text), ("flag", DataType::Boolean)],
    );

    let mut expected_by_bucket = [("even", Vec::new()), ("odd", Vec::new())];
    put_documents(
        &cassie,
        collection,
        (0..PARALLEL_ROW_COUNT).map(|index| {
            let (bucket, expected_flags) = if index % 2 == 0 {
                &mut expected_by_bucket[0]
            } else {
                &mut expected_by_bucket[1]
            };
            let flag = if index % 11 == 0 {
                None
            } else {
                Some(index != 2)
            };
            expected_flags.push(flag);
            (
                format!("doc-{index:04}"),
                serde_json::json!({
                    "bucket": bucket,
                    "flag": flag,
                }),
            )
        }),
    );

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT bucket, BOOL_AND(flag), BOOL_OR(flag), COUNT(flag) FROM exec_pg18_grouped_boolean_aggregate_states GROUP BY bucket ORDER BY bucket",
            vec![],
        )
        .expect("grouped Boolean aggregates");
    let metrics = cassie.metrics();

    // Assert
    let expected = expected_by_bucket
        .into_iter()
        .map(|(bucket, flags)| {
            let non_null = flags.iter().flatten().copied().collect::<Vec<_>>();
            let and = non_null
                .first()
                .map(|_| non_null.iter().all(|value| *value));
            let or = non_null
                .first()
                .map(|_| non_null.iter().any(|value| *value));
            vec![
                Value::String(bucket.to_owned()),
                and.map_or(Value::Null, Value::Bool),
                or.map_or(Value::Null, Value::Bool),
                Value::Int64(i64::try_from(non_null.len()).expect("count fits i64")),
            ]
        })
        .collect::<Vec<_>>();
    assert_eq!(result.rows(), expected);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 16, 16, 20]
    );
    assert!(
        metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0,
        "grouped Boolean aggregation must merge partial worker states: {metrics}"
    );
    assert!(
        metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2,
        "expected multiple aggregate workers: {metrics}"
    );

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_statistical_aggregates_over_collection_rows() {
    // Arrange
    let path = data_dir("postgresql18_statistical_aggregate_merge");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    let collection = "exec_pg18_statistical_aggregate_merge";
    let session = cassie.create_session("tester", None);
    create_registered_collection(&cassie, collection, &[("value", DataType::Float)]);
    put_documents(
        &cassie,
        collection,
        (0..PARALLEL_ROW_COUNT).map(|index| {
            let value = if index == PARALLEL_ROW_COUNT - 1 {
                serde_json::Value::Null
            } else if index % 2 == 0 {
                serde_json::json!(1.0)
            } else {
                serde_json::json!(3.0)
            };
            (
                format!("doc-{index:04}"),
                serde_json::json!({ "value": value }),
            )
        }),
    );

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT VAR_POP(value), VAR_SAMP(value), STDDEV_POP(value), STDDEV_SAMP(value), COVAR_POP(value, value), REGR_SLOPE(value, value) FROM exec_pg18_statistical_aggregate_merge",
            vec![],
        )
        .expect("parallel statistical aggregates");
    // Assert
    let actual = result.rows();
    assert_eq!(actual.len(), 1);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701; 6]
    );
    let expected = [
        1.0,
        1024.0 / 1023.0,
        1.0,
        (1024.0_f64 / 1023.0).sqrt(),
        1.0,
        1.0,
    ];
    assert_eq!(actual[0].len(), expected.len());
    for (column, expected_value) in actual[0].iter().zip(expected) {
        let Value::Float64(actual_value) = column else {
            panic!("statistical aggregate should return float8, got {column:?}");
        };
        assert!(
            (actual_value - expected_value).abs() < 1e-12,
            "expected {expected_value}, got {actual_value}"
        );
    }
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
