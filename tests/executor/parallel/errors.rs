#[test]
fn should_report_parallel_integer_sum_merge_overflow() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_aggregation_sum_overflow");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let collection = "exec_parallel_aggregation_overflow";
    let session = cassie.create_session("tester", None);
    create_registered_collection(&cassie, collection, &[("amount", DataType::BigInt)]);
    let partial_safe_amount = i64::MAX / 1024;
    put_documents(
        &cassie,
        collection,
        (0..PARALLEL_ROW_COUNT).map(|index| {
            (
                format!("doc-{index:04}"),
                serde_json::json!({ "amount": partial_safe_amount }),
            )
        }),
    );

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT SUM(amount) FROM exec_parallel_aggregation_overflow",
            vec![],
        )
        .expect_err("parallel aggregate merge should report overflow");

    // Assert
    assert!(error.to_string().contains("aggregate integer overflow"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_row_order_integer_sum_overflow_at_every_worker_count() {
    // Arrange
    let mut values = vec![serde_json::json!(0); 2050];
    values[0] = serde_json::json!(5_000_000_000_000_000_000_i64);
    values[1025] = serde_json::json!(5_000_000_000_000_000_000_i64);
    values[1026] = serde_json::json!(-9_000_000_000_000_000_000_i64);

    // Act
    let outcomes = [1, 2, 4].map(|workers| {
        aggregate_with_workers(
            "parallel_row_order_int_overflow",
            workers,
            DataType::BigInt,
            &values,
            "SELECT SUM(v) FROM exec_parallel_row_order",
        )
    });

    // Assert
    for (workers, outcome) in [1, 2, 4].into_iter().zip(outcomes) {
        let error = outcome.expect_err("row-order prefix overflows i64 at every worker count");
        assert!(
            error.contains("aggregate integer overflow"),
            "workers={workers}: {error}"
        );
    }
}

#[test]
fn should_keep_integer_sum_when_only_a_partition_local_prefix_overflows() {
    // Arrange
    let mut values = vec![serde_json::json!(0); 2050];
    values[0] = serde_json::json!(-5_000_000_000_000_000_000_i64);
    values[1025] = serde_json::json!(9_000_000_000_000_000_000_i64);
    values[1026] = serde_json::json!(5_000_000_000_000_000_000_i64);

    // Act
    let outcomes = [1, 2, 4].map(|workers| {
        aggregate_with_workers(
            "parallel_row_order_local_prefix",
            workers,
            DataType::BigInt,
            &values,
            "SELECT SUM(v) FROM exec_parallel_row_order",
        )
    });

    // Assert
    for (workers, outcome) in [1, 2, 4].into_iter().zip(outcomes) {
        assert_eq!(
            outcome.expect("row-order prefix stays within i64"),
            vec![vec![Value::Int64(9_000_000_000_000_000_000)]],
            "workers={workers}"
        );
    }
}

#[test]
fn should_keep_float_sum_when_only_a_partition_local_fold_overflows() {
    // Arrange
    let mut values = vec![serde_json::json!(0.0); 1025];
    values[0] = serde_json::json!(-1.0e308);
    values[513] = serde_json::json!(1.0e308);
    values[514] = serde_json::json!(1.0e308);

    // Act
    let outcomes = [1, 2, 4].map(|workers| {
        aggregate_with_workers(
            "parallel_row_order_float_local_overflow",
            workers,
            DataType::Float,
            &values,
            "SELECT SUM(v) FROM exec_parallel_row_order",
        )
    });

    // Assert
    for (workers, outcome) in [1, 2, 4].into_iter().zip(outcomes) {
        assert_eq!(
            outcome.expect("row-order float fold stays finite"),
            vec![vec![Value::Float64(1.0e308)]],
            "workers={workers}"
        );
    }
}
