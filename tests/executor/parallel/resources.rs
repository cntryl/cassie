#[test]
fn should_fallback_parallel_scoring_when_worker_limit_is_one() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_scoring_fallback");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_scoring_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_scoring_fallback (id TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_parallel_scoring_fallback (id, body) VALUES ('doc-1', 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT _id, search_score(body, 'alpha') AS score FROM exec_parallel_scoring_fallback WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 1",
                vec![],
            )
            .expect("fallback scoring query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(metrics["parallel_scoring"]["scorings"].as_u64(), Some(0));
        assert!(metrics["parallel_scoring"]["fallback_scorings"]
            .as_u64()
            .unwrap_or(0)
            > 0);
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_scan_when_worker_limit_is_one() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_scan_single_worker");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_scan_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_scan_single_worker";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
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
        cassie
            .midge
            .put_document(
                collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM exec_parallel_scan_single_worker",
                vec![],
            )
            .expect("single-worker query should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![vec![Value::String("alpha".to_string())]]
        );
        assert_eq!(metrics["parallel_scans"]["scans"].as_u64(), Some(0));
        assert!(
            metrics["parallel_scans"]["fallback_scans"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_having_order_limit_offset_for_parallel_aggregation() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_aggregation_having_order");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 4;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let collection = "exec_parallel_aggregation_having";
        let session = cassie.create_session("tester", None);
        create_registered_collection(
            &cassie,
            collection,
            &[("category", DataType::Text), ("score", DataType::Int)],
        );
        put_documents(
            &cassie,
            collection,
            (0..PARALLEL_ROW_COUNT).map(|index| {
                let category = match index % 4 {
                    0 => "a",
                    1 => "b",
                    2 => "c",
                    _ => "d",
                };
                (
                    format!("doc-{index:04}"),
                    serde_json::json!({
                        "category": category,
                        "score": 1,
                    }),
                )
            }),
        );

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT category, COUNT(*) AS total FROM exec_parallel_aggregation_having GROUP BY category HAVING COUNT(*) >= 256 ORDER BY category LIMIT 2 OFFSET 1",
                vec![],
            )
            .expect("parallel aggregate query should preserve downstream clauses");

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("b".to_string()), Value::Int64(256)],
                vec![Value::String("c".to_string()), Value::Int64(256)],
            ]
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fallback_parallel_aggregation_when_worker_limit_is_one() {
    // Arrange
    use_local_storage();
    let path = data_dir("parallel_aggregation_single_worker");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = 1;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE exec_parallel_aggregation_single_worker (score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO exec_parallel_aggregation_single_worker (score) VALUES (7)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS total, SUM(score) AS sum_score FROM exec_parallel_aggregation_single_worker",
                vec![],
            )
            .expect("single-worker aggregate should execute");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows(), vec![vec![Value::Int64(1), Value::Int64(7)]]);
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
            Some("typed-worker-limit-one")
        );
    });

    let _ = std::fs::remove_dir_all(path);
}
