#[test]
fn should_recover_expression_rollup_after_restart() {
    // Arrange
    use_local_storage();
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_expression_restart");
    let query = "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, MAX(amount * 10) AS maximum FROM expression_rollup_source GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY tenant";

    runtime().block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE expression_rollup_source (tenant TEXT, event_at TEXT, amount FLOAT)",
                    vec![],
                )
                .expect("create source");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO expression_rollup_source (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:05:00Z', 25.0)",
                    vec![],
                )
                .expect("insert source row");
            cassie
                .execute_sql(
                    &session,
                    "CREATE ROLLUP expression_rollup ON expression_rollup_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES MAX(amount * 10) AS maximum",
                    vec![],
                )
                .expect("create rollup");
            let metadata = cassie
                .catalog
                .get_rollup(&canonical_name("expression_rollup"))
                .expect("rollup metadata");

            // Act
            let before_restart = cassie
                .execute_sql(&session, query, vec![])
                .expect("query before restart");
            drop(cassie);

            let restarted = Cassie::new_with_data_dir(&path).expect("restart cassie");
            restarted.startup().expect("restart startup");
            let restarted_session = restarted.create_session("tester", None);
            let after_restart = restarted
                .execute_sql(&restarted_session, query, vec![])
                .expect("query after restart");
            let after_restart_rewrite_hits =
                restarted.metrics()["rollups"]["rewrite_hits"].as_u64();

            // Simulate a crash after refresh persisted Building and removed the output table.
            simulate_interrupted_rollup_refresh(&restarted, "expression_rollup");
            drop(restarted);

            let recovery = Cassie::new_with_data_dir(&path).expect("recovery cassie");
            recovery.startup().expect("recovery startup");
            let recovery_session = recovery.create_session("tester", None);
            let fallback = recovery
                .execute_sql(&recovery_session, query, vec![])
                .expect("query from authoritative source during recovery");
            let fallback_plan = recovery
                .execute_sql(&recovery_session, &format!("EXPLAIN {query}"), vec![])
                .expect("explain source fallback");
            recovery
                .execute_sql(
                    &recovery_session,
                    "REFRESH ROLLUP expression_rollup",
                    vec![],
                )
                .expect("recover interrupted rollup refresh");
            let after_recovery = recovery
                .execute_sql(&recovery_session, query, vec![])
                .expect("query rebuilt rollup");

            // Assert
            let expected_rows = vec![vec![
                Value::String("2026-01-01T00:00:00Z".to_string()),
                Value::String("a".to_string()),
                Value::Float64(250.0),
            ]];
            assert_eq!(before_restart.rows(), expected_rows);
            assert_eq!(after_restart.rows(), expected_rows);
            assert_eq!(after_restart_rewrite_hits, Some(1));
            assert_eq!(fallback.rows(), expected_rows);
            assert!(matches!(
                &fallback_plan.rows()[0][0],
                Value::String(plan) if plan.contains("rollup_rewrite=none")
            ));
            assert_eq!(after_recovery.rows(), expected_rows);
            assert_eq!(
                recovery.metrics()["rollups"]["rewrite_hits"].as_u64(),
                Some(1)
            );
            assert_eq!(metadata.version, 2);
            assert_eq!(metadata.aggregates[0].expression, "max((\"amount\" * 10))");

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_cleanup_rollup_after_restart_drop() {
    // Arrange
    use_local_storage();
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_restart");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_restart_events");
        create_hourly_rollup(&cassie, &session, "rollup_restart_events");
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let rollup_name = canonical_name("rollup_restart_events_hourly");

        // Act
        let catalog_rows = restarted
            .execute_sql(
                &session,
                &format!(
                    "SELECT rollup_name, state FROM pg_catalog.pg_rollups WHERE rollup_name = '{rollup_name}'"
                ),
                vec![],
            )
            .unwrap();
        restarted
            .execute_sql(&session, "DROP ROLLUP rollup_restart_events_hourly", vec![])
            .unwrap();

        // Assert
        assert_eq!(
            catalog_rows.rows(),
            vec![vec![
                Value::String(rollup_name.clone()),
                Value::String("ready".to_string())
            ]]
        );
        assert!(restarted.catalog.get_rollup(&rollup_name).is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}
