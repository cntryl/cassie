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
    let rollup_query = daily_incremental_query(table);
    let source_query = format!(
        "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(*) AS total, SUM(amount) AS amount_sum, MIN(amount) AS amount_min, MAX(amount) AS amount_max FROM {table} WHERE id >= 0 GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device"
    );
    let rollup = cassie.execute_sql(session, &rollup_query, vec![]).unwrap();
    let source = cassie.execute_sql(session, &source_query, vec![]).unwrap();
    let plan = cassie
        .execute_sql(session, &format!("EXPLAIN {rollup_query}"), vec![])
        .unwrap();
    assert_eq!(rollup.rows(), source.rows());
    let expected_name = canonical_name(&format!("{table}_daily"));
    assert!(matches!(
        &plan.rows()[0][0],
        Value::String(plan) if plan.contains(&format!("rollup_rewrite={expected_name}"))
    ));
}

fn incremental_groups_recomputed(cassie: &Cassie) -> u64 {
    cassie.metrics()["rollups"]["incremental_groups_recomputed"]
        .as_u64()
        .unwrap_or_default()
}

fn assert_incremental_group_delta(cassie: &Cassie, before: u64, expected: u64) {
    assert_eq!(
        incremental_groups_recomputed(cassie).saturating_sub(before),
        expected
    );
}

fn assert_last_incremental_groups(cassie: &Cassie, expected: &[(&str, &str)]) {
    let actual = cassie.metrics()["rollups"]["last_incremental_groups"]
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
        create_daily_incremental_fixture(
            &cassie,
            &session,
            "incremental_events",
        );
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
        let before = incremental_groups_recomputed(&cassie);
        cassie
            .execute_sql(
                &session,
                "UPDATE incremental_events SET amount = 6 WHERE id = 2",
                vec![],
            )
            .unwrap();
        assert_incremental_group_delta(&cassie, before, 1);
        assert_last_incremental_groups(&cassie, &[("a", "2026-01-01T00:00:00Z")]);
        assert_daily_incremental_parity(&cassie, &session, "incremental_events");

        let before = incremental_groups_recomputed(&cassie);
        cassie
            .execute_sql(
                &session,
                "INSERT INTO incremental_events VALUES (5, 'a', '2025-12-31T23:00:00Z', 3)",
                vec![],
            )
            .unwrap();
        assert_incremental_group_delta(&cassie, before, 1);
        assert_last_incremental_groups(&cassie, &[("a", "2025-12-31T00:00:00Z")]);
        assert_daily_incremental_parity(&cassie, &session, "incremental_events");

        let before = incremental_groups_recomputed(&cassie);
        cassie
            .execute_sql(
                &session,
                "UPDATE incremental_events SET device = 'b', event_at = '2026-01-02T00:00:00Z' WHERE id = 1",
                vec![],
            )
            .unwrap();
        assert_incremental_group_delta(&cassie, before, 2);
        assert_last_incremental_groups(
            &cassie,
            &[
                ("a", "2026-01-01T00:00:00Z"),
                ("b", "2026-01-02T00:00:00Z"),
            ],
        );
        assert_daily_incremental_parity(&cassie, &session, "incremental_events");

        let before = incremental_groups_recomputed(&cassie);
        cassie
            .execute_sql(
                &session,
                "DELETE FROM incremental_events WHERE id = 3",
                vec![],
            )
            .unwrap();
        assert_incremental_group_delta(&cassie, before, 1);
        assert_last_incremental_groups(&cassie, &[("b", "2026-01-01T00:00:00Z")]);
        assert_daily_incremental_parity(&cassie, &session, "incremental_events");

        let before = incremental_groups_recomputed(&cassie);
        cassie
            .execute_sql(
                &session,
                "INSERT INTO incremental_events VALUES (6, 'a', '2026-01-01T05:00:00Z', NULL)",
                vec![],
            )
            .unwrap();
        assert_incremental_group_delta(&cassie, before, 1);
        assert_last_incremental_groups(&cassie, &[("a", "2026-01-01T00:00:00Z")]);
        assert_daily_incremental_parity(&cassie, &session, "incremental_events");

        let before = incremental_groups_recomputed(&cassie);
        for _ in 0..2 {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO incremental_events VALUES (6, 'a', '2026-01-01T05:00:00Z', NULL) ON CONFLICT (id) DO UPDATE SET amount = EXCLUDED.amount",
                    vec![],
                )
                .unwrap();
            assert_incremental_group_delta(&cassie, before, 1);
            assert_last_incremental_groups(&cassie, &[("a", "2026-01-01T00:00:00Z")]);
            assert_daily_incremental_parity(&cassie, &session, "incremental_events");
        }

        let before = incremental_groups_recomputed(&cassie);
        cassie.execute_sql(&session, "BEGIN", vec![]).unwrap();
        cassie
            .execute_sql(
                &session,
                "UPDATE incremental_events SET amount = 99 WHERE id = 2",
                vec![],
            )
            .unwrap();
        cassie.execute_sql(&session, "ROLLBACK", vec![]).unwrap();

        // Assert
        assert_incremental_group_delta(&cassie, before, 0);
        assert_last_incremental_groups(&cassie, &[]);
        assert_daily_incremental_parity(&cassie, &session, "incremental_events");
    });
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

        drop(recovered);
        let _ = std::fs::remove_dir_all(path);
    });
}
