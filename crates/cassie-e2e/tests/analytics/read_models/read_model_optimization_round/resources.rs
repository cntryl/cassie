#[test]
fn should_stop_vectorized_join_after_unordered_limit_budget() {
    // Arrange
    let path = data_dir("read_model_vectorized_join_limit_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_limit_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_limit_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        let mut orders = Vec::new();
        for index in 0..64 {
            users.push((
                Some(format!("user-{index:02}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:02}"),
                }),
            ));
            orders.push((
                Some(format!("order-{index:02}")),
                serde_json::json!({
                    "order_user_key": i64::from(index),
                    "total": i64::from(index),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_limit_users"), users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_limit_orders"), orders)
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_limit_users.name, read_model_limit_orders.total \
                 FROM read_model_limit_users JOIN read_model_limit_orders \
                 ON read_model_limit_users.user_key = read_model_limit_orders.order_user_key \
                 LIMIT 5",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 5);
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        let output_delta = after["joins"]["output_rows_total"].as_u64().unwrap()
            - before["joins"]["output_rows_total"].as_u64().unwrap();
        let probe_delta = after["joins"]["vectorized_probe_rows_total"]
            .as_u64()
            .unwrap()
            - before["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap();
        assert_eq!(output_delta, 5);
        assert!(
            probe_delta <= 8,
            "expected limited join to probe at most one vectorized batch, got {probe_delta}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_push_unordered_left_join_limit_into_left_source_scan() {
    // Arrange
    let path = data_dir("read_model_left_join_source_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        // The bounded source, qualified rows, and joined output overlap.
        // 64 KiB fits that working set; an eager 200-row source still cannot fit.
        config.limits.query_memory_budget_bytes = 64 * 1024;
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_budget_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_budget_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_budget_users"), users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(
                &canonical_collection("read_model_budget_orders"),
                vec![(
                    Some("order-000".to_string()),
                    serde_json::json!({
                        "order_user_key": 0_i64,
                        "total": 42_i64,
                    }),
                )],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_budget_users.name, read_model_budget_orders.total \
                 FROM read_model_budget_users LEFT JOIN read_model_budget_orders \
                 ON read_model_budget_users.user_key = read_model_budget_orders.order_user_key \
                 LIMIT 5",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 5);
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("typed_hash"));
        let probe_delta = after["joins"]["vectorized_probe_rows_total"]
            .as_u64()
            .unwrap()
            - before["joins"]["vectorized_probe_rows_total"]
                .as_u64()
                .unwrap();
        assert!(
            probe_delta <= 8,
            "expected limited left join to scan at most one left-source batch, got {probe_delta}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_null_trailing_index_keys_in_bounded_indexed_join() {
    // Arrange
    let path = data_dir("read_model_indexed_join_null_trailing_key");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie =
                Cassie::new_in_memory_with_config(vectorized_join_config()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE indexed_join_nullable_users (user_key INT, name TEXT)",
                "CREATE TABLE indexed_join_nullable_orders (order_user_key INT, total INT)",
                "CREATE INDEX indexed_join_nullable_users_idx \
                 ON indexed_join_nullable_users USING btree (user_key, name)",
                "INSERT INTO indexed_join_nullable_users (user_key, name) VALUES (150, NULL)",
                "INSERT INTO indexed_join_nullable_users (user_key, name) VALUES (151, 'named')",
                "INSERT INTO indexed_join_nullable_orders (order_user_key, total) VALUES (150, 150)",
                "INSERT INTO indexed_join_nullable_orders (order_user_key, total) VALUES (151, 151)",
            ] {
                cassie.execute_sql(&session, sql, vec![]).expect(sql);
            }

            // Act
            let mut rows = cassie
                .execute_sql(
                    &session,
                    "SELECT indexed_join_nullable_users.name, indexed_join_nullable_orders.total \
                 FROM indexed_join_nullable_users JOIN indexed_join_nullable_orders \
                 ON indexed_join_nullable_users.user_key = indexed_join_nullable_orders.order_user_key \
                 LIMIT 2",
                    vec![],
                )
                .unwrap()
                .rows().to_vec();
            rows.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));

            // Assert
            assert_eq!(
                rows,
                vec![
                    vec![Value::Null, Value::Int64(150)],
                    vec![Value::String("named".to_string()), Value::Int64(151)],
                ]
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_probe_indexed_left_source_for_bounded_inner_join() {
    // Arrange
    let path = data_dir("read_model_indexed_inner_join_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        config.limits.vectorized_join_batch_size = 8;
        // Admit the two indexed rows, source conversion, and retained output together.
        config.limits.query_memory_budget_bytes = 64 * 1024;
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_indexed_join_tables(&cassie, &session);
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_indexed_users.name, read_model_indexed_orders.total \
                 FROM read_model_indexed_users JOIN read_model_indexed_orders \
                 ON read_model_indexed_users.user_key = read_model_indexed_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_indexed_join_result(&result, &before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_stream_unindexed_bounded_inner_join_until_output_budget() {
    // Arrange
    let path = data_dir("read_model_streaming_inner_join_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        // Preserve the original preemptive dense path while admitting retained state.
        config.limits.vectorized_join_batch_size = 64;
        config.limits.query_memory_budget_bytes = 64 * 1024;
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_stream_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_stream_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_stream_users"), users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(
                &canonical_collection("read_model_stream_orders"),
                vec![
                    (
                        Some("order-000".to_string()),
                        serde_json::json!({
                            "order_user_key": 0_i64,
                            "total": 10_i64,
                        }),
                    ),
                    (
                        Some("order-001".to_string()),
                        serde_json::json!({
                            "order_user_key": 1_i64,
                            "total": 11_i64,
                        }),
                    ),
                ],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_stream_users.name, read_model_stream_orders.total \
                 FROM read_model_stream_users JOIN read_model_stream_orders \
                 ON read_model_stream_users.user_key = read_model_stream_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(10)],
                vec![Value::String("user-001".to_string()), Value::Int64(11)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        assert_eq!(
            after["joins"]["last_bounded_side_selection_reason"].as_str(),
            Some("dense_stream_preemptive_temp_budget")
        );
        let scanned_delta = after["read_paths"]["collection_scan_rows"]
            .as_u64()
            .unwrap()
            - before["read_paths"]["collection_scan_rows"]
                .as_u64()
                .unwrap();
        assert!(
            scanned_delta <= 10,
            "expected streaming bounded join to avoid full left materialization, scanned {scanned_delta} rows"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_stream_dense_bounded_inner_join_without_materializing_right_source() {
    // Arrange
    let path = data_dir("read_model_dense_streaming_inner_join_budget");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = vectorized_join_config();
        // Keep budget <= 2 * batch_size * 512 so this remains the preemptive dense path.
        // The capacity admits both current source rows plus retained joined output.
        config.limits.vectorized_join_batch_size = 64;
        config.limits.query_memory_budget_bytes = 64 * 1024;
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_dense_stream_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_dense_stream_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();

        let mut users = Vec::new();
        let mut orders = Vec::new();
        for index in 0..200 {
            users.push((
                Some(format!("user-{index:03}")),
                serde_json::json!({
                    "user_key": i64::from(index),
                    "name": format!("user-{index:03}"),
                }),
            ));
            orders.push((
                Some(format!("order-{index:03}")),
                serde_json::json!({
                    "order_user_key": 0_i64,
                    "total": i64::from(index),
                }),
            ));
        }
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_dense_stream_users"), users)
            .unwrap();
        cassie
            .midge
            .put_fresh_documents(&canonical_collection("read_model_dense_stream_orders"), orders)
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_dense_stream_users.name, read_model_dense_stream_orders.total \
                 FROM read_model_dense_stream_users JOIN read_model_dense_stream_orders \
                 ON read_model_dense_stream_users.user_key = read_model_dense_stream_orders.order_user_key \
                 LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("user-000".to_string()), Value::Int64(0)],
                vec![Value::String("user-000".to_string()), Value::Int64(1)],
            ]
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("vectorized"));
        assert_eq!(
            after["joins"]["last_bounded_side_selection_reason"].as_str(),
            Some("dense_stream_preemptive_temp_budget")
        );
        let scanned_delta = after["read_paths"]["collection_scan_rows"]
            .as_u64()
            .unwrap()
            - before["read_paths"]["collection_scan_rows"]
                .as_u64()
                .unwrap();
        assert!(
            scanned_delta <= 6,
            "expected dense bounded join to avoid materializing either source, scanned {scanned_delta} rows"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
