#[test]
fn should_isolate_runtime_feedback_by_offset_shape() {
    // Arrange
    let path = data_dir("feedback_offset_shape");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let collection = "metrics_feedback_offset_shape";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let without_offset_sql =
            "SELECT title FROM metrics_feedback_offset_shape ORDER BY title LIMIT 1";
        let with_offset_sql =
            "SELECT title FROM metrics_feedback_offset_shape ORDER BY title LIMIT 1 OFFSET 500";
        let without_offset_key = feedback_key(&cassie, &session, without_offset_sql, None);
        let with_offset_key = feedback_key(&cassie, &session, with_offset_sql, None);

        // Act
        cassie
            .execute_sql(&session, without_offset_sql, vec![])
            .expect("execute query without OFFSET");
        cassie
            .execute_sql(&session, with_offset_sql, vec![])
            .expect("execute query with OFFSET");
        let without_offset_record = cassie
            .feedback_record_for_diagnostics(&without_offset_key)
            .expect("feedback without OFFSET");
        let with_offset_record = cassie
            .feedback_record_for_diagnostics(&with_offset_key)
            .expect("feedback with OFFSET");

        // Assert
        assert_ne!(
            without_offset_key.predicate_shape_hash,
            with_offset_key.predicate_shape_hash
        );
        assert_eq!(without_offset_record.executions, 1);
        assert_eq!(with_offset_record.executions, 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_capture_runtime_feedback_for_normalized_select() {
    // Arrange
    let path = data_dir("feedback_capture");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let collection = "metrics_feedback_capture";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_capture WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, None);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let metrics = cassie.metrics();
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("scan feedback should be recorded");

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(record.executions, 1);
        assert_eq!(record.rows_out_total, 1);
        assert_eq!(record.errors_total, 0);
        assert!(
            metrics["feedback"]["writes"].as_u64().unwrap_or_default() >= 1,
            "feedback writes should be tracked"
        );
        assert!(
            metrics["feedback"]["misses"].as_u64().unwrap_or_default() >= 1,
            "first feedback lookup should miss"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_capture_runtime_feedback_for_selected_index() {
    // Arrange
    let path = data_dir("feedback_selected_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let collection = "metrics_feedback_selected_index";
        let index_name = "metrics_feedback_selected_title_idx";
        register_feedback_collection(&cassie, collection);
        let index = IndexMeta {
            collection: collection.to_string(),
            name: index_name.to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::default(),
        };
        cassie.midge.put_index(&index).unwrap();
        cassie.catalog.register_index(index);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT body FROM metrics_feedback_selected_index WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, Some(index_name));

        // Act
        let result = cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let explained = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM metrics_feedback_selected_index WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("selected index feedback should be recorded");

        // Assert
        assert_eq!(result.rows().len(), 1);
        assert_eq!(record.executions, 1);
        assert_eq!(record.rows_out_total, 1);
        assert_eq!(key.operator_family, "index_read");
        let cassie::types::Value::String(plan) = &explained.rows()[0][0] else {
            panic!("expected explain string");
        };
        assert!(plan.contains("index_feedback=enabled"));
        assert!(plan.contains(index_name));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_skip_feedback_lookup_when_execution_result_cache_hits() {
    // Arrange
    let path = data_dir("feedback_result_cache_hit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE feedback_result_cache_hit (title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO feedback_result_cache_hit (title) VALUES ('alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "SELECT title FROM feedback_result_cache_hit WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let after_first = cassie.metrics();
        let second = cassie
            .execute_sql(
                &session,
                "SELECT title FROM feedback_result_cache_hit WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let after_second = cassie.metrics();

        // Assert
        assert_eq!(first.rows(), second.rows());
        assert_eq!(
            after_second["feedback"]["hits"].as_u64(),
            after_first["feedback"]["hits"].as_u64()
        );
        assert_eq!(
            after_second["feedback"]["misses"].as_u64(),
            after_first["feedback"]["misses"].as_u64()
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_partition_runtime_feedback_by_schema_epoch() {
    // Arrange
    let path = data_dir("feedback_schema_epoch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let collection = "metrics_feedback_schema_epoch";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_schema_epoch WHERE title = $1";
        let first_key = feedback_key(&cassie, &session, sql, None);

        // Act
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE feedback_schema_marker (id INT)",
                vec![],
            )
            .unwrap();
        let second_key = feedback_key(&cassie, &session, sql, None);
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .unwrap();
        let first = cassie.feedback_record_for_diagnostics(&first_key).is_none();
        let second = cassie
            .feedback_record_for_diagnostics(&second_key)
            .expect("second schema epoch feedback");
        let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

        // Assert
        assert!(first, "schema changes should invalidate older feedback");
        assert_eq!(second.executions, 1);
        assert!(
            persisted
                .iter()
                .all(|(key, _)| key.schema_epoch == second_key.schema_epoch),
            "persisted feedback must not retain an invalid schema epoch: {persisted:?}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_operator_feedback_when_confidence_is_too_low() {
    // Arrange
    let path = data_dir("operator_feedback_low_confidence");
    let config = operator_feedback_config(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_in_memory_with_config(config).unwrap();
            let collection = "metrics_operator_feedback_low_confidence";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);
            let sql = "SELECT title FROM metrics_operator_feedback_low_confidence WHERE title = $1";
            let key = feedback_key(&cassie, &session, sql, None);

            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String("alpha".to_string())],
                )
                .unwrap();

            // Act
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM metrics_operator_feedback_low_confidence WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let plan = explain.rows()[0][0].as_str().unwrap().to_string();
            let record = cassie
                .feedback_record_for_diagnostics(&key)
                .expect("low-confidence feedback");

            // Assert
            assert!(record.confidence_bps < 600, "record={record:?}");
            assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
            assert!(
                plan.contains("operator_feedback_reason=low_confidence"),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_use_confident_operator_feedback_to_switch_selected_index() {
    // Arrange
    let path = data_dir("operator_feedback_switch");
    let config = operator_feedback_config(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_in_memory_with_config(config).unwrap();
            let collection = "metrics_operator_feedback_switch";
            let base_index = "metrics_operator_feedback_body_idx_a";
            let preferred_index = "metrics_operator_feedback_title_idx_b";
            register_feedback_collection(&cassie, collection);
            register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
            let session = cassie.create_session("tester", None);
            let explain_sql = "EXPLAIN SELECT title FROM metrics_operator_feedback_switch WHERE title = 'alpha' AND body = 'one'";
            let shape_sql = "SELECT title FROM metrics_operator_feedback_switch WHERE title = 'alpha' AND body = 'one'";
            let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
            let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));

            let baseline = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
            let baseline_plan = baseline.rows()[0][0].as_str().unwrap().to_string();
            assert!(baseline_plan.contains(base_index), "plan={baseline_plan}");

            for _ in 0..4 {
                cassie
                    .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                    .expect("seed base feedback");
                cassie
                    .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                    .expect("seed preferred feedback");
            }
            let base_record = cassie
                .feedback_record_for_diagnostics(&base_key)
                .expect("base record");
            let preferred_record = cassie
                .feedback_record_for_diagnostics(&preferred_key)
                .expect("preferred record");
            assert_ne!(base_key, preferred_key);
            assert!(
                preferred_record.stable_average_elapsed_ms()
                    < base_record.stable_average_elapsed_ms()
            );

            // Act
            let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
            let plan = explain.rows()[0][0].as_str().unwrap().to_string();

            // Assert
            assert!(plan.contains(preferred_index), "plan={plan}");
            assert!(plan.contains("operator_feedback=used"), "plan={plan}");
            assert!(
                plan.contains(&format!(
                    "operator_feedback_base_candidate=index:{base_index}"
                )),
                "plan={plan}"
            );
            assert!(
                plan.contains(&format!(
                    "operator_feedback_selected_candidate=index:{preferred_index}"
                )),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_fall_back_to_base_index_when_operator_feedback_is_disabled() {
    // Arrange
    let path = data_dir("operator_feedback_disabled");
    let config = operator_feedback_config(false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_in_memory_with_config(config).unwrap();
            let collection = "metrics_operator_feedback_disabled";
            let base_index = "metrics_operator_feedback_disabled_body_idx_a";
            let preferred_index = "metrics_operator_feedback_disabled_title_idx_b";
            register_feedback_collection(&cassie, collection);
            register_operator_feedback_indexes(&cassie, collection, base_index, preferred_index);
            let session = cassie.create_session("tester", None);
            let explain_sql = "EXPLAIN SELECT title FROM metrics_operator_feedback_disabled WHERE title = 'alpha' AND body = 'one'";
            let shape_sql = "SELECT title FROM metrics_operator_feedback_disabled WHERE title = 'alpha' AND body = 'one'";
            let base_key = feedback_key(&cassie, &session, shape_sql, Some(base_index));
            let preferred_key = feedback_key(&cassie, &session, shape_sql, Some(preferred_index));

            for _ in 0..4 {
                cassie
                    .seed_feedback_for_diagnostics(&base_key, &confident_feedback(90, 24))
                    .expect("seed base feedback");
                cassie
                    .seed_feedback_for_diagnostics(&preferred_key, &confident_feedback(5, 1))
                    .expect("seed preferred feedback");
            }

            // Act
            let explain = cassie.execute_sql(&session, explain_sql, vec![]).unwrap();
            let plan = explain.rows()[0][0].as_str().unwrap().to_string();

            // Assert
            assert!(plan.contains(base_index), "plan={plan}");
            assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
            assert!(
                plan.contains("operator_feedback_reason=disabled"),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_report_stale_operator_feedback_in_explain_diagnostics() {
    // Arrange
    let path = data_dir("operator_feedback_stale");
    let mut config = operator_feedback_config(true);
    config.limits.feedback_ttl_seconds = 1;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config).unwrap();
        let collection = "metrics_operator_feedback_stale";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_operator_feedback_stale WHERE title = $1";

        for value in ["alpha", "beta", "alpha", "beta"] {
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String(value.to_string())],
                )
                .unwrap();
        }
        std::thread::sleep(Duration::from_millis(1_200));

        // Act
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM metrics_operator_feedback_stale WHERE title = 'alpha'",
                vec![],
            )
            .unwrap();
        let plan = explain.rows()[0][0].as_str().unwrap().to_string();

        // Assert
        assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
        assert!(
            plan.contains("operator_feedback_reason=stale"),
            "plan={plan}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_runtime_feedback_in_explain_analyze_output() {
    // Arrange
    let path = data_dir("feedback_explain_analyze");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
            let collection = "metrics_feedback_explain_analyze";
            register_feedback_collection(&cassie, collection);
            let session = cassie.create_session("tester", None);

            // Act
            let explain = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN ANALYZE SELECT title FROM metrics_feedback_explain_analyze WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let plan = explain.rows()[0][0].as_str().unwrap().to_string();
            let metrics = cassie.metrics();

            // Assert
            assert!(plan.contains("analyze=true"), "plan={plan}");
            assert!(plan.contains("operator_actuals=Scan:"), "plan={plan}");
            assert!(plan.contains("rows_out:1"), "plan={plan}");
            assert!(
                metrics["feedback"]["writes"].as_u64().unwrap_or_default() >= 1,
                "EXPLAIN ANALYZE should write feedback"
            );

            let _ = std::fs::remove_dir_all(path);
        });
}
