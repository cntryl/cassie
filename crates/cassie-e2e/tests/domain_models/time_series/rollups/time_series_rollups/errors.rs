#[test]
fn should_reject_non_finite_rollup_output_before_publication() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_non_finite_output");

    runtime().block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE non_finite_rollup_source (tenant TEXT, event_at TEXT, amount FLOAT)",
                    vec![],
                )
                .expect("create source");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO non_finite_rollup_source (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:05:00Z', 1e308)",
                    vec![],
                )
                .expect("insert source row");

            // Act
            let create = cassie.execute_sql(
                &session,
                "CREATE ROLLUP non_finite_rollup ON non_finite_rollup_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES MAX(amount * 10) AS maximum",
                vec![],
            );
            let source_rows = cassie
                .execute_sql(
                    &session,
                    "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, MAX(amount * 10) AS maximum FROM non_finite_rollup_source GROUP BY time_bucket('1 hour', event_at), tenant",
                    vec![],
                )
                .expect("read authoritative source result");
            let rollup_name = canonical_name("non_finite_rollup");
            let output_name = output_collection_name(&rollup_name);

            // Assert
            assert!(
                create.is_err(),
                "non-finite rollup output must be rejected before publication"
            );
            assert!(cassie.catalog.get_rollup(&rollup_name).is_none());
            assert!(cassie.midge.collection_schema(&output_name).is_none());
            assert!(matches!(
                source_rows.rows(),
                [row] if matches!(row.get(2), Some(Value::Float64(value)) if value.is_infinite())
            ));

            let _ = std::fs::remove_dir_all(path);
        });
}

#[test]
fn should_reject_rollup_with_mismatched_source_generation() {
    // Arrange
    let path = data_dir("rollup_generation_fence");

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_generation_events");
        create_hourly_rollup(&cassie, &session, "rollup_generation_events");
        let mut rollup = cassie
            .catalog
            .get_rollup(&canonical_name("rollup_generation_events_hourly"))
            .expect("rollup metadata");
        rollup.refresh_cursor.source_generation = 0;
        cassie.midge.put_rollup(&rollup).unwrap();
        cassie.catalog.register_rollup(rollup);

        // Act
        let result = cassie
            .execute_sql(&session, &hourly_query("rollup_generation_events"), vec![])
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(result.rows().len(), 2);
        assert_eq!(metrics["rollups"]["rewrite_hits"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}
