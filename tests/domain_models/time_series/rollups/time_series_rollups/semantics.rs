#[test]
fn should_rewrite_query_after_rollup_creation() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_rewrite_events");
        create_hourly_rollup(&cassie, &session, "rollup_rewrite_events");

        // Act
        let selected = cassie
            .execute_sql(&session, &hourly_query("rollup_rewrite_events"), vec![])
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                &format!("EXPLAIN {}", hourly_query("rollup_rewrite_events")),
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        let rollup_name = canonical_name("rollup_rewrite_events_hourly");
        assert_eq!(
            selected.rows(),
            vec![
                vec![
                    Value::String("2026-01-01T00:00:00Z".to_string()),
                    Value::String("a".to_string()),
                    Value::Int64(2),
                    Value::Int64(12)
                ],
                vec![
                    Value::String("2026-01-01T01:00:00Z".to_string()),
                    Value::String("b".to_string()),
                    Value::Int64(1),
                    Value::Int64(3)
                ],
            ]
        );
        assert_eq!(metrics["rollups"]["rewrite_hits"].as_u64(), Some(1));
        assert!(matches!(
            &explain.rows()[0][0],
            Value::String(plan) if plan.contains(&format!("rollup_rewrite={rollup_name}"))
        ));
        assert!(cassie.catalog.get_rollup(&rollup_name).is_some());

    });
}

#[test]
fn should_preserve_rollup_output_after_non_finite_refresh_failure() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();

    runtime().block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE non_finite_refresh_source (tenant TEXT, event_at TEXT, amount FLOAT)",
                    vec![],
                )
                .expect("create source");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO non_finite_refresh_source (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:05:00Z', 1e307)",
                    vec![],
                )
                .expect("insert source row");
            cassie
                .execute_sql(
                    &session,
                    "CREATE ROLLUP non_finite_refresh_rollup ON non_finite_refresh_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES MAX(amount * 10) AS maximum",
                    vec![],
                )
                .expect("create finite rollup");
            let rollup_name = canonical_name("non_finite_refresh_rollup");
            let output_name = output_collection_name(&rollup_name);
            let output_local_name = output_name.rsplit('.').next().expect("rollup local name");
            let output_query = format!("SELECT maximum FROM {output_local_name}");
            let previous_output = cassie
                .execute_sql(&session, &output_query, vec![])
                .expect("read previous rollup output");

            // Act
            cassie
                .execute_sql(
                    &session,
                    "UPDATE non_finite_refresh_source SET amount = 1e308 WHERE tenant = 'a'",
                    vec![],
                )
                .expect("source update remains committed");
            let source_query = "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, MAX(amount * 10) AS maximum FROM non_finite_refresh_source GROUP BY time_bucket('1 hour', event_at), tenant";
            let authoritative = cassie
                .execute_sql(&session, source_query, vec![])
                .expect("read authoritative source result");
            let output_after = cassie
                .execute_sql(&session, &output_query, vec![])
                .expect("read retained rollup output");
            let metadata = cassie
                .catalog
                .get_rollup(&rollup_name)
                .expect("rollup metadata");
            let source_generation = cassie
                .midge
                .collection_generation("non_finite_refresh_source")
                .expect("source generation");
            let debt = cassie
                .execute_sql(
                    &session,
                    "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.non_finite_refresh_source'",
                    vec![],
                )
                .expect("read maintenance debt");

            // Assert
            assert!(matches!(
                previous_output.rows(),
                [row] if matches!(row.first(), Some(Value::Float64(value)) if value.is_finite() && *value > 9.9e307)
            ));
            assert_eq!(output_after.rows(), previous_output.rows());
            assert_eq!(metadata.state, RollupState::Ready);
            assert!(!metadata.is_fresh(source_generation));
            assert_eq!(debt.rows(), vec![vec![Value::String("rollup".to_string())]]);
            assert!(matches!(
                authoritative.rows(),
                [row] if matches!(row.get(2), Some(Value::Float64(value)) if value.is_infinite())
            ));

            });
}

#[test]
fn should_fail_closed_for_pre_0_1_rollup_definition_version() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "legacy_rollup_events");
        create_hourly_rollup(&cassie, &session, "legacy_rollup_events");
        let rollup_name = canonical_name("legacy_rollup_events_hourly");
        let mut metadata = cassie
            .catalog
            .get_rollup(&rollup_name)
            .expect("rollup metadata");
        metadata.version = 2;
        cassie
            .midge
            .put_rollup(&metadata)
            .expect("persist pre-0.1.0 format");
        cassie.catalog.register_rollup(metadata);

        // Act
        let query = hourly_query("legacy_rollup_events");
        let selected = cassie
            .execute_sql(&session, &query, vec![])
            .expect("source fallback query");
        let explain = cassie
            .execute_sql(&session, &format!("EXPLAIN {query}"), vec![])
            .expect("explain fallback query");
        let refresh = cassie.execute_sql(
            &session,
            "REFRESH ROLLUP legacy_rollup_events_hourly",
            vec![],
        );

        // Assert
        assert_eq!(selected.rows().len(), 2);
        assert!(matches!(
            &explain.rows()[0][0],
            Value::String(plan) if plan.contains("rollup_rewrite=none")
        ));
        assert!(refresh.is_err());
        assert_eq!(
            cassie.metrics()["rollups"]["rewrite_hits"].as_u64(),
            Some(0)
        );

    });
}

#[test]
fn should_not_rewrite_to_a_rollup_after_a_grouped_column_name_is_reused() {
    // Arrange

    runtime().block_on(async {
            let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE rollup_column_rename_events (tenant TEXT, other TEXT, event_at TEXT, amount INT)",
                    vec![],
                )
                .expect("create source");
            for sql in [
                "INSERT INTO rollup_column_rename_events (tenant, other, event_at, amount) VALUES ('a', 'X', '2026-01-01T00:05:00Z', 1)",
                "INSERT INTO rollup_column_rename_events (tenant, other, event_at, amount) VALUES ('b', 'Y', '2026-01-01T01:05:00Z', 2)",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("insert source row");
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE ROLLUP rollup_column_rename_hourly ON rollup_column_rename_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
                    vec![],
                )
                .expect("create rollup");

            // Act
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE rollup_column_rename_events RENAME COLUMN tenant TO tmpname",
                    vec![],
                )
                .expect("rename original grouping column");
            cassie
                .execute_sql(
                    &session,
                    "ALTER TABLE rollup_column_rename_events RENAME COLUMN other TO tenant",
                    vec![],
                )
                .expect("reuse grouping column name");
            let query = "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) AS amount_sum FROM rollup_column_rename_events GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY bucket, tenant";
            let selected = cassie
                .execute_sql(&session, query, vec![])
                .expect("query after rename");
            let explain = cassie
                .execute_sql(&session, &format!("EXPLAIN {query}"), vec![])
                .expect("explain query after rename");

            // Assert
            assert_eq!(
                selected.rows(),
                vec![
                    vec![
                        Value::String("2026-01-01T00:00:00Z".to_string()),
                        Value::String("X".to_string()),
                        Value::Int64(1),
                        Value::Int64(1),
                    ],
                    vec![
                        Value::String("2026-01-01T01:00:00Z".to_string()),
                        Value::String("Y".to_string()),
                        Value::Int64(1),
                        Value::Int64(2),
                    ],
                ]
            );
            assert!(matches!(
                &explain.rows()[0][0],
                Value::String(plan) if plan.contains("rollup_rewrite=none")
            ));

            });
}

#[test]
fn should_serialize_concurrent_rollup_publication_after_source_change() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let (cassie, source, rollup, initial_generation) = create_rollup_publication_fixture();
    let updated_generation = initial_generation.saturating_add(1);

    let first_paused = std::sync::Arc::new(std::sync::Barrier::new(2));
    let first_resume = std::sync::Arc::new(std::sync::Barrier::new(2));
    let newer_paused = std::sync::Arc::new(std::sync::Barrier::new(2));
    let newer_resume = std::sync::Arc::new(std::sync::Barrier::new(2));
    cassie::executor::set_rollup_publication_before_replace_barriers(
        Some(rollup.clone()),
        initial_generation,
        Some(std::sync::Arc::clone(&first_paused)),
        Some(std::sync::Arc::clone(&first_resume)),
    );
    cassie::executor::set_rollup_publication_before_ready_barriers(
        Some(rollup.clone()),
        updated_generation,
        Some(std::sync::Arc::clone(&newer_paused)),
        Some(std::sync::Arc::clone(&newer_resume)),
    );

    // Act
    let first_cassie = std::sync::Arc::clone(&cassie);
    let (first_finished_sender, first_finished_receiver) = std::sync::mpsc::channel();
    let first = std::thread::spawn(move || {
        let session = first_cassie.create_session("first refresh", None);
        let result =
            first_cassie.execute_sql(&session, "REFRESH ROLLUP rollup_publication_hourly", vec![]);
        first_finished_sender
            .send(())
            .expect("signal first refresh completion");
        result
    });
    first_paused.wait();

    let writer_cassie = std::sync::Arc::clone(&cassie);
    let writer = std::thread::spawn(move || {
        let session = writer_cassie.create_session("source writer", None);
        writer_cassie.execute_sql(
            &session,
            "INSERT INTO rollup_publication_events VALUES ('a', '2026-01-01T00:15:00Z', 100)",
            vec![],
        )
    });
    let generation_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while cassie.midge.collection_generation(&source).unwrap() < updated_generation
        && std::time::Instant::now() < generation_deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let observed_generation = cassie.midge.collection_generation(&source).unwrap();
    if observed_generation < updated_generation {
        first_resume.wait();
        let _ = first.join().expect("first refresh thread");
        let _ = writer.join().expect("source writer thread");
        panic!("source write did not advance the collection generation");
    }

    let release_first = std::sync::Arc::clone(&first_resume);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        release_first.wait();
    });
    newer_paused.wait();
    releaser.join().expect("first refresh release thread");
    first_finished_receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("older refresh should finish before the newer publication resumes");
    newer_resume.wait();
    let first_result = first.join().expect("first refresh thread");
    let writer_result = writer.join().expect("source writer thread");
    let session = cassie.create_session("verify", None);
    let selected = cassie
        .execute_sql(&session, &hourly_query("rollup_publication_events"), vec![])
        .unwrap();
    let metadata = cassie.catalog.get_rollup(&rollup).expect("rollup metadata");

    // Assert
    assert!(
        first_result.is_err(),
        "older refresh should detect the newer source generation"
    );
    writer_result.expect("source write and maintenance refresh should succeed");
    assert_eq!(metadata.state, RollupState::Ready);
    assert_eq!(
        metadata.refresh_cursor.source_generation,
        updated_generation
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("2026-01-01T00:00:00Z".to_string()),
            Value::String("a".to_string()),
            Value::Int64(2),
            Value::Int64(101),
        ]]
    );
}

#[test]
fn should_refresh_rollup_for_dml_movement() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_dml_events");
        create_hourly_rollup(&cassie, &session, "rollup_dml_events");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO rollup_dml_events (tenant, event_at, amount) VALUES ('a', '2026-01-01T01:30:00Z', 11)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE rollup_dml_events SET event_at = '2026-01-01T02:05:00Z' WHERE amount = 11",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "DELETE FROM rollup_dml_events WHERE tenant = 'b'",
                vec![],
            )
            .unwrap();
        let selected = cassie
            .execute_sql(&session, &hourly_query("rollup_dml_events"), vec![])
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![
                vec![
                    Value::String("2026-01-01T00:00:00Z".to_string()),
                    Value::String("a".to_string()),
                    Value::Int64(2),
                    Value::Int64(12)
                ],
                vec![
                    Value::String("2026-01-01T02:00:00Z".to_string()),
                    Value::String("a".to_string()),
                    Value::Int64(1),
                    Value::Int64(11)
                ],
            ]
        );

    });
}

#[test]
fn should_fallback_to_source_when_rollup_is_stale() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_stale_events");
        create_hourly_rollup(&cassie, &session, "rollup_stale_events");
        let mut meta = cassie
            .catalog
            .get_rollup("rollup_stale_events_hourly")
            .unwrap();
        meta.state = RollupState::Stale;
        meta.refresh_cursor.lag_rows = 1;
        cassie.midge.put_rollup(&meta).unwrap();
        cassie.catalog.register_rollup(meta);

        // Act
        let selected = cassie
            .execute_sql(&session, &hourly_query("rollup_stale_events"), vec![])
            .unwrap();
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(selected.rows().len(), 2);
        assert_eq!(metrics["rollups"]["stale_fallbacks"].as_u64(), Some(1));

    });
}

#[test]
fn should_record_rollup_debt_after_refresh_failure() {
    // Arrange
    let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_maintenance_events");
        create_hourly_rollup(&cassie, &session, "rollup_maintenance_events");
        inject_rollup_refresh_failure(&cassie, &session, "rollup_maintenance_events");

        // Act
        let source_rows = cassie
            .execute_sql(
                &session,
                &hourly_query("rollup_maintenance_events"),
                vec![],
            )
            .unwrap();
        let debt = cassie
            .execute_sql(
                &session,
                "SELECT artifact, target_generation, retry_count, last_error, fallback_reason FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.rollup_maintenance_events'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(source_rows.rows().len(), 3, "{:?}", source_rows.rows());
        assert_eq!(source_rows.rows()[0][2], Value::Int64(2));
        assert_eq!(source_rows.rows()[0][3], Value::Int64(12));
        assert_eq!(debt.rows().len(), 1);
        assert_eq!(
            debt.rows()[0],
            vec![
                Value::String("rollup".to_string()),
                Value::Int64(4),
                Value::Int64(1),
                Value::String("rollup maintenance failed (details redacted)".to_string()),
                Value::String("maintenance_pending".to_string()),
            ]
        );
        assert_eq!(
            cassie.metrics()["rollups"]["last_fallback_reason"].as_str(),
            Some("maintenance_pending")
        );

    });
}

#[test]
fn should_retry_rollup_debt_on_startup() {
    // Arrange
    use_local_storage();
    let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_maintenance_restart");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_restart_maintenance_events");
        create_hourly_rollup(&cassie, &session, "rollup_restart_maintenance_events");
        inject_rollup_refresh_failure(
            &cassie,
            &session,
            "rollup_restart_maintenance_events",
        );

        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let recovered = restarted
            .execute_sql(
                &restarted_session,
                &hourly_query("rollup_restart_maintenance_events"),
                vec![],
            )
            .unwrap();
        let remaining_debt = restarted
            .execute_sql(
                &restarted_session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.rollup_restart_maintenance_events'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(recovered.rows().len(), 3);
        assert_eq!(recovered.rows()[0][2], Value::Int64(2));
        assert_eq!(recovered.rows()[0][3], Value::Int64(12));
        assert_eq!(remaining_debt.rows(), [] as [std::vec::Vec<cassie::types::Value>; 0]);

    });
}

#[test]
fn should_move_rollup_debt_with_collection_rename() {
    // Arrange
    let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();

    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        seed_events(&cassie, &session, "rollup_rename_events");
        create_hourly_rollup(&cassie, &session, "rollup_rename_events");
        inject_rollup_refresh_failure(&cassie, &session, "rollup_rename_events");
        assert!(cassie
            .midge
            .has_rollup_maintenance_debt("rollup_rename_events")
            .unwrap());

        // Act
        cassie
            .midge
            .rename_collection("rollup_rename_events", "rollup_renamed_events")
            .unwrap();

        // Assert
        assert!(!cassie
            .midge
            .has_rollup_maintenance_debt("rollup_rename_events")
            .unwrap());
        assert!(cassie
            .midge
            .has_rollup_maintenance_debt("rollup_renamed_events")
            .unwrap());

    });
}

#[test]
fn should_keep_rollup_sum_of_integer_column_in_bigint() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE ev (tenant TEXT, event_at TEXT, amount INT, small SMALLINT)",
            "INSERT INTO ev (tenant, event_at, amount, small) VALUES ('a', '2026-01-01T00:05:00Z', 1, 1)",
            "CREATE ROLLUP ev_hourly ON ev USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum, SUM(small) AS small_sum",
            "INSERT INTO ev (tenant, event_at, amount, small) VALUES ('a', '2026-01-01T00:20:00Z', 2000000000, 30000)",
            "INSERT INTO ev (tenant, event_at, amount, small) VALUES ('a', '2026-01-01T00:40:00Z', 2000000000, 30000)",
        ] {
            assert!(cassie.execute_sql(&session, sql, vec![]).is_ok(), "{sql}");
        }

    // Act
    let refreshed = cassie
        .execute_sql(&session, "REFRESH ROLLUP ev_hourly", vec![])
        .is_ok();
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT total, amount_sum, small_sum FROM __cassie_rollup_ev_hourly",
            vec![],
        )
        .map(|result| result.rows().to_vec())
        .unwrap_or_default();

    // Assert
    assert!(refreshed, "REFRESH ROLLUP must succeed");
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(3),
            Value::Int64(4_000_000_001),
            Value::Int64(60_001)
        ]]
    );
}
