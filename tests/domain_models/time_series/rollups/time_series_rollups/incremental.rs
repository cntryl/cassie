fn create_daily_incremental_fixture(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    table: &str,
) {
    for sql in [
        format!(
            "CREATE TABLE {table} (id INT PRIMARY KEY, device TEXT NOT NULL, event_at TIMESTAMP NOT NULL, amount INT)"
        ),
        format!(
            "CREATE INDEX {table}_time ON {table} USING time_series (event_at) WITH (bucket_width = '1 day', partition_by = device)"
        ),
    ] {
        cassie.execute_sql(session, &sql, vec![]).unwrap();
    }
}

fn create_daily_incremental_rollup(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    table: &str,
    rollup: &str,
) {
    cassie
        .execute_sql(
            session,
            &format!(
                "CREATE ROLLUP {rollup} ON {table} USING time_bucket('1 day', event_at) GROUP BY device AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum, MIN(amount) AS amount_min, MAX(amount) AS amount_max"
            ),
            vec![],
        )
        .unwrap();
}

fn daily_incremental_query(table: &str) -> String {
    format!(
        "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(*) AS total, SUM(amount) AS amount_sum, MIN(amount) AS amount_min, MAX(amount) AS amount_max FROM {table} GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device"
    )
}

fn assert_daily_incremental_parity(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    table: &str,
) {
    assert_daily_incremental_rows_match_source(cassie, session, table);
    let rollup_query = daily_incremental_query(table);
    let plan = cassie
        .execute_sql(session, &format!("EXPLAIN {rollup_query}"), vec![])
        .unwrap();
    let expected_name = canonical_name(&format!("{table}_daily"));
    assert!(matches!(
        &plan.rows()[0][0],
        Value::String(plan) if plan.contains(&format!("rollup_rewrite={expected_name}"))
    ));
}

fn assert_daily_incremental_rows_match_source(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    table: &str,
) {
    let rollup_query = daily_incremental_query(table);
    let source_query = format!(
        "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(*) AS total, SUM(amount) AS amount_sum, MIN(amount) AS amount_min, MAX(amount) AS amount_max FROM {table} WHERE id >= 0 GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device"
    );
    let rollup = cassie.execute_sql(session, &rollup_query, vec![]).unwrap();
    let source = cassie.execute_sql(session, &source_query, vec![]).unwrap();
    assert_eq!(rollup.rows(), source.rows());
}

fn incremental_groups_recomputed(cassie: &Cassie) -> u64 {
    cassie.metrics()["rollups"]["incremental_groups_recomputed"]
        .as_u64()
        .unwrap_or_default()
}

fn assert_no_incremental_fallback(cassie: &Cassie) {
    assert_eq!(
        cassie.metrics()["rollups"]["last_incremental_fallback_reason"],
        ""
    );
}

fn assert_incremental_group_delta(cassie: &Cassie, before: u64, expected: u64) {
    assert_eq!(
        incremental_groups_recomputed(cassie).saturating_sub(before),
        expected
    );
}

fn assert_last_incremental_groups(cassie: &Cassie, expected: &[(&str, &str)]) {
    let metrics = cassie.metrics();
    let actual = metrics["rollups"]["last_incremental_groups"]
        .as_array()
        .expect("last affected group identities");
    let actual = actual
        .iter()
        .map(|group| {
            (
                group["device"].as_str().expect("group device").to_owned(),
                group["bucket"].as_str().expect("group bucket").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    let expected = expected
        .iter()
        .map(|(device, bucket)| ((*device).to_owned(), (*bucket).to_owned()))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn should_recompute_only_changed_daily_groups_after_committed_mutations() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_daily_incremental_fixture(&cassie, &session, "incremental_events");
        for sql in [
            "INSERT INTO incremental_events VALUES (1, 'a', '2026-01-01T01:00:00Z', 1)",
            "INSERT INTO incremental_events VALUES (2, 'a', '2026-01-01T02:00:00Z', 4)",
            "INSERT INTO incremental_events VALUES (3, 'b', '2026-01-01T03:00:00Z', 2)",
            "INSERT INTO incremental_events VALUES (4, 'unrelated', '2026-01-04T00:00:00Z', 10)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        create_daily_incremental_rollup(
            &cassie,
            &session,
            "incremental_events",
            "incremental_events_daily",
        );

        // Act
        let baseline = incremental_groups_recomputed(&cassie);

        // Assert
        assert_daily_incremental_mutation_contract(&cassie, &session, baseline);
    });
}

fn assert_daily_incremental_mutation_contract(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    baseline: u64,
) {
    assert_update_and_insert_changes(cassie, session, baseline);
    assert_move_delete_duplicate_and_rollback_changes(cassie, session);
}

fn assert_update_and_insert_changes(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    baseline: u64,
) {
    cassie
        .execute_sql(
            session,
            "UPDATE incremental_events SET amount = 6 WHERE id = 2",
            vec![],
        )
        .unwrap();
    assert_incremental_group_delta(cassie, baseline, 1);
    assert_last_incremental_groups(cassie, &[("a", "2026-01-01T00:00:00Z")]);
    assert_daily_incremental_parity(cassie, session, "incremental_events");
    assert_no_incremental_fallback(cassie);

    let before = incremental_groups_recomputed(cassie);
    cassie
        .execute_sql(
            session,
            "INSERT INTO incremental_events VALUES (5, 'a', '2025-12-31T23:00:00Z', 3)",
            vec![],
        )
        .unwrap();
    assert_incremental_group_delta(cassie, before, 1);
    assert_last_incremental_groups(cassie, &[("a", "2025-12-31T00:00:00Z")]);
    assert_daily_incremental_parity(cassie, session, "incremental_events");
}

fn assert_move_delete_duplicate_and_rollback_changes(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
) {
    let before = incremental_groups_recomputed(cassie);
    cassie
        .execute_sql(
            session,
            "UPDATE incremental_events SET device = 'b', event_at = '2026-01-02T00:00:00Z' WHERE id = 1",
            vec![],
        )
        .unwrap();
    assert_incremental_group_delta(cassie, before, 2);
    assert_last_incremental_groups(
        cassie,
        &[("a", "2026-01-01T00:00:00Z"), ("b", "2026-01-02T00:00:00Z")],
    );
    assert_daily_incremental_parity(cassie, session, "incremental_events");

    let before = incremental_groups_recomputed(cassie);
    cassie
        .execute_sql(
            session,
            "DELETE FROM incremental_events WHERE id = 3",
            vec![],
        )
        .unwrap();
    assert_incremental_group_delta(cassie, before, 1);
    assert_last_incremental_groups(cassie, &[("b", "2026-01-01T00:00:00Z")]);
    assert_daily_incremental_parity(cassie, session, "incremental_events");

    let before = incremental_groups_recomputed(cassie);
    cassie
        .execute_sql(
            session,
            "INSERT INTO incremental_events VALUES (6, 'a', '2026-01-01T05:00:00Z', NULL)",
            vec![],
        )
        .unwrap();
    assert_incremental_group_delta(cassie, before, 1);
    assert_last_incremental_groups(cassie, &[("a", "2026-01-01T00:00:00Z")]);
    assert_daily_incremental_parity(cassie, session, "incremental_events");

    for _ in 0..2 {
        let before = incremental_groups_recomputed(cassie);
        cassie
            .execute_sql(
                session,
                "INSERT INTO incremental_events VALUES (6, 'a', '2026-01-01T05:00:00Z', NULL) ON CONFLICT (id) DO UPDATE SET amount = EXCLUDED.amount",
                vec![],
            )
            .unwrap();
        assert_incremental_group_delta(cassie, before, 1);
        assert_last_incremental_groups(cassie, &[("a", "2026-01-01T00:00:00Z")]);
        assert_daily_incremental_parity(cassie, session, "incremental_events");
    }

    let before = incremental_groups_recomputed(cassie);
    cassie.execute_sql(session, "BEGIN", vec![]).unwrap();
    cassie
        .execute_sql(
            session,
            "UPDATE incremental_events SET amount = 99 WHERE id = 2",
            vec![],
        )
        .unwrap();
    cassie.execute_sql(session, "ROLLBACK", vec![]).unwrap();

    // A rolled-back write leaves the rollup stale until the next committed
    // write, so this read is answered from the authoritative source.
    assert_incremental_group_delta(cassie, before, 0);
    assert_last_incremental_groups(cassie, &[("a", "2026-01-01T00:00:00Z")]);
    assert_daily_incremental_rows_match_source(cassie, session, "incremental_events");
}

#[test]
fn should_keep_daily_recompute_work_constant_as_unrelated_partitions_grow_tenfold() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_daily_incremental_fixture(
            &cassie,
            &session,
            "incremental_scale_events",
        );
        cassie
            .execute_sql(
                &session,
                "INSERT INTO incremental_scale_events VALUES (1, 'selected', '2026-01-01T01:00:00Z', 1)",
                vec![],
            )
            .unwrap();
        for index in 0..9 {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO incremental_scale_events VALUES ({}, 'noise-{index}', '2026-02-01T01:00:00Z', 1)",
                        index + 2
                    ),
                    vec![],
                )
                .unwrap();
        }
        create_daily_incremental_rollup(
            &cassie,
            &session,
            "incremental_scale_events",
            "incremental_scale_events_daily",
        );

        // Act
        let before = incremental_groups_recomputed(&cassie);
        cassie
            .execute_sql(
                &session,
                "UPDATE incremental_scale_events SET amount = 2 WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_incremental_group_delta(&cassie, before, 1);
        assert_last_incremental_groups(&cassie, &[("selected", "2026-01-01T00:00:00Z")]);
        for index in 0..90 {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO incremental_scale_events VALUES ({}, 'expanded-{index}', '2026-03-01T01:00:00Z', 1)",
                        index + 20
                    ),
                    vec![],
                )
                .unwrap();
        }
        let before = incremental_groups_recomputed(&cassie);
        cassie
            .execute_sql(
                &session,
                "UPDATE incremental_scale_events SET amount = 3 WHERE id = 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_incremental_group_delta(&cassie, before, 1);
        assert_last_incremental_groups(&cassie, &[("selected", "2026-01-01T00:00:00Z")]);
        assert_daily_incremental_parity(&cassie, &session, "incremental_scale_events");
        assert_no_incremental_fallback(&cassie);
    });
}

#[test]
fn should_fallback_to_authoritative_rows_then_clear_rollup_debt_after_restart() {
    // Arrange
    use_local_storage();
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_incremental_debt");
    let query = daily_incremental_query("incremental_debt_events");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_daily_incremental_fixture(
            &cassie,
            &session,
            "incremental_debt_events",
        );
        cassie
            .execute_sql(
                &session,
                "INSERT INTO incremental_debt_events VALUES (1, 'a', '2026-01-01T01:00:00Z', 1)",
                vec![],
            )
            .unwrap();
        create_daily_incremental_rollup(
            &cassie,
            &session,
            "incremental_debt_events",
            "incremental_debt_events_daily",
        );
        set_rollup_maintenance_failure_point(true);

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE incremental_debt_events SET amount = 8 WHERE id = 1",
                vec![],
            )
            .unwrap();
        let fallback = cassie.execute_sql(&session, &query, vec![]).unwrap();
        let fallback_plan = cassie
            .execute_sql(&session, &format!("EXPLAIN {query}"), vec![])
            .unwrap();
        let debt = cassie
            .execute_sql(
                &session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.incremental_debt_events'",
                vec![],
            )
            .unwrap();
        let expected = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(*) AS total, SUM(amount) AS amount_sum, MIN(amount) AS amount_min, MAX(amount) AS amount_max FROM incremental_debt_events WHERE id >= 0 GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device",
                vec![],
            )
            .unwrap();
        drop(cassie);

        let recovered = Cassie::new_with_data_dir(&path).unwrap();
        recovered.startup().unwrap();
        let recovered_session = recovered.create_session("tester", None);
        let after_restart = recovered
            .execute_sql(&recovered_session, &query, vec![])
            .unwrap();
        let after_debt = recovered
            .execute_sql(
                &recovered_session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.incremental_debt_events'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(fallback.rows(), expected.rows());
        assert!(matches!(
            &fallback_plan.rows()[0][0],
            Value::String(plan) if plan.contains("rollup_rewrite=none")
        ));
        assert_eq!(debt.rows(), vec![vec![Value::String("rollup".to_string())]]);
        assert_eq!(after_restart.rows(), expected.rows());
        assert!(after_debt.rows().is_empty());
        assert_eq!(
            recovered.metrics()["rollups"]["last_incremental_fallback_reason"],
            "change-report-unavailable"
        );

        drop(recovered);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_skip_rollup_recompute_for_a_duplicate_projection_replay_event() {
    // Arrange
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_daily_incremental_fixture(&cassie, &session, "incremental_replay_events");
        create_daily_incremental_rollup(
            &cassie,
            &session,
            "incremental_replay_events",
            "incremental_replay_events_daily",
        );
        let mut projection =
            ::cassie::catalog::ProjectionMeta::new(canonical_name("incremental_replay_events"), 1);
        projection.source_identity = Some("incremental-event-stream".to_owned());
        cassie.midge.put_projection_metadata(&projection).unwrap();
        cassie.catalog.register_projection_metadata(projection);
        let batch = cassie::app::ProjectionReplayBatch {
            projection: canonical_name("incremental_replay_events"),
            source_identity: "incremental-event-stream".to_owned(),
            batch_id: "incremental-event-1".to_owned(),
            lag: 0,
            events: vec![cassie::app::ProjectionReplayEvent {
                event_id: "incremental-event-1".to_owned(),
                checkpoint: "checkpoint-1".to_owned(),
                position: Some(1),
                document_id: "1".to_owned(),
                payload: Some(serde_json::json!({
                    "id": 1,
                    "device": "replayed",
                    "event_at": "2026-01-01T01:00:00Z",
                    "amount": 7
                })),
            }],
        };

        // Act
        let first_before = incremental_groups_recomputed(&cassie);
        let first = cassie.replay_projection_batch(batch.clone()).unwrap();
        let duplicate_before = incremental_groups_recomputed(&cassie);
        let duplicate = cassie.replay_projection_batch(batch).unwrap();

        // Assert
        assert_eq!(first.applied_event_count, 1);
        assert_eq!(duplicate.skipped_duplicate_count, 1);
        assert_eq!(
            incremental_groups_recomputed(&cassie).saturating_sub(first_before),
            1
        );
        assert_eq!(
            incremental_groups_recomputed(&cassie).saturating_sub(duplicate_before),
            0
        );
        assert_last_incremental_groups(&cassie, &[("replayed", "2026-01-01T00:00:00Z")]);
        assert_daily_incremental_parity(&cassie, &session, "incremental_replay_events");
        assert_no_incremental_fallback(&cassie);
    });
}

#[test]
fn should_record_group_limit_fallback_after_a_bounded_source_report() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE incremental_group_limit (id INT PRIMARY KEY, d TEXT NOT NULL, t TIMESTAMP NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX incremental_group_limit_time ON incremental_group_limit USING time_series (t) WITH (bucket_width = '1 day', partition_by = d)",
                vec![],
            )
            .unwrap();
        let values = (0..129)
            .map(|id| format!("({id}, 'o{id}', '2026-01-01T00:00:00Z')"))
            .collect::<Vec<_>>()
            .join(", ");
        let insert = format!("INSERT INTO incremental_group_limit VALUES {values}");
        cassie.execute_sql(&session, &insert, vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE ROLLUP incremental_group_limit_daily ON incremental_group_limit USING time_bucket('1 day', t) GROUP BY d AGGREGATES COUNT(*) AS total",
                vec![],
            )
            .unwrap();
        let cases = (0..129)
            .map(|id| format!("WHEN {id} THEN 'n{id}'"))
            .collect::<Vec<_>>()
            .join(" ");
        let update = format!(
            "UPDATE incremental_group_limit SET d = CASE id {cases} ELSE d END"
        );

        // Act
        cassie.execute_sql(&session, &update, vec![]).unwrap();
        let rollup = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 day', t) AS bucket, d, COUNT(*) AS total FROM incremental_group_limit GROUP BY time_bucket('1 day', t), d ORDER BY bucket, d",
                vec![],
            )
            .unwrap();
        let source = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 day', t) AS bucket, d, COUNT(*) AS total FROM incremental_group_limit WHERE id >= 0 GROUP BY time_bucket('1 day', t), d ORDER BY bucket, d",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(rollup.rows(), source.rows());
        assert_eq!(rollup.rows().len(), 129);
        assert_eq!(
            cassie.metrics()["rollups"]["incremental_groups_recomputed"],
            0
        );
        assert_eq!(
            cassie.metrics()["rollups"]["last_incremental_fallback_reason"],
            "affected-group-limit"
        );
    });
}

#[test]
fn should_record_change_report_size_fallback_for_many_rows_in_one_group() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_daily_incremental_fixture(&cassie, &session, "incremental_report_limit");
        create_daily_incremental_rollup(
            &cassie,
            &session,
            "incremental_report_limit",
            "incremental_report_limit_daily",
        );
        let values = (1..=600)
            .map(|id| format!("({id}, 'a', '2026-01-01T00:00:00Z', 1)"))
            .collect::<Vec<_>>()
            .join(", ");
        let insert = format!("INSERT INTO incremental_report_limit VALUES {values}");

        // Act
        cassie.execute_sql(&session, &insert, vec![]).unwrap();
        let rollup = cassie
            .execute_sql(
                &session,
                &daily_incremental_query("incremental_report_limit"),
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(rollup.rows().len(), 1);
        assert_eq!(rollup.rows()[0][2], Value::Int64(600));
        assert_eq!(
            cassie.metrics()["rollups"]["last_incremental_fallback_reason"],
            "change-report-limit"
        );
    });
}

#[test]
fn should_record_unsupported_aggregate_fallback_reason() {
    // Arrange
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_daily_incremental_fixture(&cassie, &session, "incremental_unsupported");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO incremental_unsupported VALUES (1, 'a', '2026-01-01T01:00:00Z', 1)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE ROLLUP incremental_unsupported_daily ON incremental_unsupported USING time_bucket('1 day', event_at) GROUP BY device AGGREGATES COUNT(amount) AS amount_count",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "UPDATE incremental_unsupported SET amount = 9 WHERE id = 1",
                vec![],
            )
            .unwrap();
        let rollup = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(amount) AS amount_count FROM incremental_unsupported GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device",
                vec![],
            )
            .unwrap();
        let source = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(amount) AS amount_count FROM incremental_unsupported WHERE id >= 0 GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(rollup.rows(), source.rows());
        assert_eq!(
            cassie.metrics()["rollups"]["last_incremental_fallback_reason"],
            "unsupported-rollup-definition"
        );
    });
}
