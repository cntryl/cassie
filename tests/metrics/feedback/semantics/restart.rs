#[test]
fn should_persist_runtime_feedback_delta_without_replacing_unrelated_records() {
    // Arrange
    let path = data_dir("feedback_delta_persistence");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let collection = "metrics_feedback_delta_persistence";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let sql = "SELECT title FROM metrics_feedback_delta_persistence WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, None);
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("alpha".to_string())],
            )
            .unwrap();
        let current = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("current feedback record");
        let unrelated_key = RuntimeFeedbackKey {
            collection: "unrelated_persisted_feedback".to_string(),
            predicate_shape_hash: key.predicate_shape_hash.wrapping_add(1),
            ..key.clone()
        };
        let unrelated_record = RuntimeFeedbackRecord {
            executions: 17,
            first_seen_ms: 1,
            last_seen_ms: 2,
            ..RuntimeFeedbackRecord::default()
        };
        cassie
            .midge
            .replace_runtime_feedback_records(&[
                (key.clone(), current),
                (unrelated_key.clone(), unrelated_record),
            ])
            .expect("seed persisted feedback records");

        // Act
        cassie
            .execute_sql(
                &session,
                sql,
                vec![cassie::types::Value::String("beta".to_string())],
            )
            .unwrap();
        let persisted = cassie.midge.list_runtime_feedback_records().unwrap();

        // Assert
        let unrelated = persisted
            .iter()
            .find(|(persisted_key, _)| persisted_key == &unrelated_key)
            .map(|(_, record)| record)
            .expect("unrelated persisted feedback must remain untouched");
        assert_eq!(unrelated.executions, 17);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reconcile_runtime_feedback_after_delta_persistence_failure() {
    // Arrange
    let path = data_dir("feedback_delta_retry");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let collection = "metrics_feedback_delta_retry";
        register_feedback_collection(&cassie, collection);
        let session = cassie.create_session("tester", None);
        let first_sql = "SELECT title FROM metrics_feedback_delta_retry";
        let second_sql = "SELECT body FROM metrics_feedback_delta_retry";
        let first_key = feedback_key(&cassie, &session, first_sql, None);
        let second_key = feedback_key(&cassie, &session, second_sql, None);
        let before_errors = cassie.metrics()["storage"]["schema"]["errors"]
            .as_u64()
            .unwrap_or_default();
        set_operator_feedback_persistence_failure_point(true);

        // Act
        cassie.execute_sql(&session, first_sql, vec![]).unwrap();
        let after_failure = cassie.midge.list_runtime_feedback_records().unwrap();
        cassie.execute_sql(&session, second_sql, vec![]).unwrap();
        let after_reconciliation = cassie.midge.list_runtime_feedback_records().unwrap();
        let after_errors = cassie.metrics()["storage"]["schema"]["errors"]
            .as_u64()
            .unwrap_or_default();

        // Assert
        assert!(cassie.feedback_record_for_diagnostics(&first_key).is_some());
        assert!(
            after_failure
                .iter()
                .all(|(persisted_key, _)| persisted_key != &first_key),
            "failed delta must not partially commit: {after_failure:?}"
        );
        assert_eq!(after_errors, before_errors.saturating_add(1));
        assert!(
            [&first_key, &second_key].iter().all(|expected| {
                after_reconciliation
                    .iter()
                    .any(|(persisted_key, _)| persisted_key == *expected)
            }),
            "reconciled={after_reconciliation:?}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_ignore_malformed_persisted_feedback_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("feedback_malformed_restart");
    let config = operator_feedback_config(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE metrics_feedback_malformed_restart (title TEXT)",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO metrics_feedback_malformed_restart (title) VALUES ('alpha')",
                    vec![],
                )
                .unwrap();
            cassie
                .execute_sql(
                    &session,
                    "SELECT title FROM metrics_feedback_malformed_restart WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap();
            let feedback_storage_key = cassie
                .midge
                .raw_scan_prefix(StorageFamily::Schema, b"")
                .unwrap()
                .into_iter()
                .find_map(|(key, value)| {
                    serde_json::from_slice::<serde_json::Value>(&value)
                        .ok()
                        .filter(|record| record.get("record").is_some() && record.get("key").is_some())
                        .map(|_| key)
                })
                .expect("persisted runtime feedback entry");
            cassie
                .midge
                .raw_put(StorageFamily::Schema, &feedback_storage_key, b"malformed-feedback")
                .unwrap();
            drop(session);
            drop(cassie);

            // Act
            let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);
            let plan = cassie
                .execute_sql(
                    &session,
                    "EXPLAIN SELECT title FROM metrics_feedback_malformed_restart WHERE title = 'alpha'",
                    vec![],
                )
                .unwrap()
                .rows()[0][0]
                .as_str()
                .unwrap()
                .to_string();

            // Assert
            assert!(cassie.midge.list_runtime_feedback_records().unwrap().is_empty());
            assert!(plan.contains("operator_feedback=ignored"), "plan={plan}");
            assert!(
                plan.contains("operator_feedback_reason=missing"),
                "plan={plan}"
            );

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_hydrate_persisted_operator_feedback_from_storage() {
    // Arrange
    let path = data_dir("operator_feedback_restart");
    let config = operator_feedback_config(true);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE metrics_operator_feedback_restart (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        for (title, body) in [("alpha", "one"), ("beta", "two"), ("gamma", "three")] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO metrics_operator_feedback_restart (title, body) VALUES ($1, $2)",
                    vec![
                        cassie::types::Value::String(title.to_string()),
                        cassie::types::Value::String(body.to_string()),
                    ],
                )
                .unwrap();
        }
        let sql = "SELECT title FROM metrics_operator_feedback_restart WHERE title = $1";
        let key = feedback_key(&cassie, &session, sql, None);

        for value in ["alpha", "beta", "gamma"] {
            cassie
                .execute_sql(
                    &session,
                    sql,
                    vec![cassie::types::Value::String(value.to_string())],
                )
                .unwrap();
        }
        assert!(
            !cassie
                .midge
                .list_runtime_feedback_records()
                .unwrap()
                .is_empty(),
            "feedback records should be persisted into storage"
        );
        cassie.clear_feedback_for_diagnostics();
        assert!(
            cassie.feedback_record_for_diagnostics(&key).is_none(),
            "clearing runtime feedback should remove the in-memory record"
        );

        cassie
            .reload_feedback_from_storage_for_diagnostics()
            .expect("reload feedback from storage");

        // Act
        let record = cassie
            .feedback_record_for_diagnostics(&key)
            .expect("persisted feedback record");

        // Assert
        assert_eq!(record.executions, 3, "record={record:?}");
        assert!(record.confidence_bps >= 600, "record={record:?}");

        let _ = std::fs::remove_dir_all(path);
    });
}
