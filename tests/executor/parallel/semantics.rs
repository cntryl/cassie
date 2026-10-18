#[test]
fn should_score_fulltext_candidates_with_parallel_workers() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_scoring_fulltext");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_scoring_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_scoring_fulltext";
        let session = cassie.create_session("tester", None);
        create_registered_collection(
            &cassie,
            collection,
            &[("id", DataType::Text), ("body", DataType::Text)],
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "id": format!("doc-{index:04}"),
                        "body": "alpha beta",
                    }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT _id, search_score(body, 'alpha') AS score FROM exec_parallel_scoring_fulltext WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 3",
                vec![],
            )
            .expect("parallel scoring query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 3);
        assert!(metrics["parallel_scoring"]["scorings"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert!(metrics["parallel_scoring"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2);
        assert!(metrics["parallel_scoring"]["rows"].as_u64().unwrap_or(0) > 0);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_merge_parallel_scan_batches_deterministically() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_scan_merge");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_scan_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_scan_merge";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "id": format!("doc-{index:04}"),
                        "title": format!("title-{index:04}"),
                    }),
                )
            }),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(&session, "SELECT * FROM exec_parallel_scan_merge", vec![])
            .expect("parallel scan query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), PARALLEL_ROW_COUNT);
        assert!(metrics["parallel_scans"]["scans"].as_u64().unwrap_or(0) > 0);
        assert!(metrics["parallel_scans"]["workers"].as_u64().unwrap_or(0) >= 2);
        assert_eq!(
            metrics["parallel_scans"]["rows"].as_u64(),
            Some(PARALLEL_ROW_COUNT as u64)
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_grouped_aggregates_with_parallel_workers() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_aggregation_grouped");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_grouped";
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "category".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "score".to_string(),
                    data_type: DataType::Int,
                    nullable: true,
                },
            ],
        };
        cassie
            .midge
            .create_collection(collection, schema.clone())
            .unwrap();
        cassie.register_collection(
            collection,
            schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = if index % 2 == 0 { "even" } else { "odd" };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "category": category,
                        "score": 1,
                    }),
                )
            }),
        );
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total, SUM(score) AS sum_score FROM exec_parallel_aggregation_grouped GROUP BY category ORDER BY category",
                vec![],
            )
            .expect("parallel aggregate query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![
                    Value::String("even".to_string()),
                    Value::Int64(513),
                    Value::Int64(513),
                ],
                vec![
                    Value::String("odd".to_string()),
                    Value::Int64(512),
                    Value::Int64(512),
                ],
            ]
        );
        assert!(metrics["parallel_aggregation"]["aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert!(metrics["parallel_aggregation"]["workers"]
            .as_u64()
            .unwrap_or(0)
            >= 2);
        assert_eq!(
            metrics["parallel_aggregation"]["rows"].as_u64(),
            Some(PARALLEL_ROW_COUNT as u64)
        );
        assert_eq!(metrics["parallel_aggregation"]["groups"].as_u64(), Some(2));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_row_order_for_parallel_aggregate_queries_with_average() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_aggregation_ungrouped_nulls");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_nulls";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("score", DataType::Int)]);
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let score = if index % 10 == 0 {
                    serde_json::Value::Null
                } else {
                    serde_json::json!(1)
                };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({ "score": score }),
                )
            }),
        );
        let non_null_rows = (0..PARALLEL_ROW_COUNT)
            .filter(|index| index % 10 != 0)
            .count();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS total, COUNT(score) AS non_null, SUM(score) AS sum_score, AVG(score) AS avg_score, MIN(score) AS min_score, MAX(score) AS max_score FROM exec_parallel_aggregation_nulls",
                vec![],
            )
            .expect("parallel ungrouped aggregate should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![
                Value::Int64(parallel_row_count_i64()),
                Value::Int64(usize_to_i64(non_null_rows)),
                Value::Int64(usize_to_i64(non_null_rows)),
                Value::Float64(1.0),
                Value::Int64(1),
                Value::Int64(1),
            ]]
        );
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(metrics["parallel_aggregation"]["fallback_aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("typed-row-order-fold")
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_aggregation_for_distinct() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_aggregation_distinct_fallback");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_distinct";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("category", DataType::Text)]);
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = if index % 2 == 0 { "even" } else { "odd" };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({ "category": category }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT category, COUNT(*) AS total FROM exec_parallel_aggregation_distinct GROUP BY category ORDER BY category",
                vec![],
            )
            .expect("distinct aggregate fallback should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 2);
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(metrics["parallel_aggregation"]["fallback_aggregations"]
            .as_u64()
            .unwrap_or(0)
            > 0);
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("distinct")
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_aggregation_for_user_defined_function() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_aggregation_udf_fallback");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_udf";
        let session = cassie.create_session("tester", None);
        create_registered_collection(&cassie, collection, &[("score", DataType::Int)]);
        cassie
            .execute_sql(
                &session,
                "CREATE FUNCTION agg_identity(x INT) RETURNS INT AS \"x\"",
                vec![],
            )
            .unwrap();
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT)
                .map(|index| (format!("doc-{index:04}"), serde_json::json!({ "score": 1 }))),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT SUM(agg_identity(score)) AS total FROM exec_parallel_aggregation_udf",
                vec![],
            )
            .expect("udf aggregate fallback should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![Value::Int64(parallel_row_count_i64())]]
        );
        assert_eq!(
            metrics["parallel_aggregation"]["aggregations"].as_u64(),
            Some(0)
        );
        assert!(
            metrics["parallel_aggregation"]["fallback_aggregations"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
        assert_eq!(
            metrics["parallel_aggregation"]["last_fallback_reason"].as_str(),
            Some("unsupported-expression")
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fold_float_aggregates_in_row_order_at_every_worker_count() {
    // Arrange
    let mut values = vec![serde_json::json!(1.0); 1025];
    values[0] = serde_json::json!(1.0e16);

    // Act
    let outcomes = [1, 2, 4].map(|workers| {
        aggregate_with_workers(
            "parallel_row_order_float",
            workers,
            DataType::Float,
            &values,
            "SELECT SUM(v), AVG(v) FROM exec_parallel_row_order",
        )
    });

    // Assert
    for (workers, outcome) in [1, 2, 4].into_iter().zip(outcomes) {
        assert_eq!(
            outcome.expect("float aggregate should execute"),
            vec![vec![
                Value::Float64(1.0e16),
                Value::Float64(1.0e16 / 1025.0)
            ]],
            "workers={workers}"
        );
    }
}

#[test]
fn should_fold_integer_avg_in_row_order_beyond_exact_float_range() {
    // Arrange
    let mut values = vec![serde_json::json!(1); 1025];
    values[0] = serde_json::json!(1_i64 << 60);

    // Act
    let outcomes = [1, 2, 4].map(|workers| {
        aggregate_with_workers(
            "parallel_row_order_int_avg",
            workers,
            DataType::BigInt,
            &values,
            "SELECT AVG(v), SUM(v) FROM exec_parallel_row_order",
        )
    });

    // Assert
    let row_order_avg = (1..1025).fold(2.0_f64.powi(60), |sum, _| sum + 1.0) / 1025.0;
    for (workers, outcome) in [1, 2, 4].into_iter().zip(outcomes) {
        assert_eq!(
            outcome.expect("integer average should execute"),
            vec![vec![
                Value::Float64(row_order_avg),
                Value::Int64((1_i64 << 60) + 1024)
            ]],
            "workers={workers}"
        );
    }
}
