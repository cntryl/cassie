fn scheduled_policy_progress(
    cassie: &Cassie,
    session: &CassieSession,
    policy: &str,
) -> Vec<Vec<Value>> {
    let policy_name = canonical_name(policy);
    let result = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT schedule_state, next_due_at, pending_cutoff, pending_scanned_rows, pending_deleted_rows, pending_skipped_rows, last_successful_cutoff, last_error, last_failure, next_due_at::TEXT, pending_cutoff::TEXT, last_successful_cutoff::TEXT, enforcement_mode, state FROM pg_catalog.pg_retention_policies WHERE policy_name = '{policy_name}'"
            ),
            vec![],
        )
        .expect("scheduled retention progress");
    result.rows().to_vec()
}

#[test]
fn should_hydrate_legacy_retention_metadata_as_unscheduled() {
    // Arrange
    let path = data_dir("retention_legacy_metadata");
    let cassie = Cassie::new_with_data_dir(&path).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE legacy_events (id INT PRIMARY KEY, event_at TIMESTAMP)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &session,
            "CREATE RETENTION POLICY legacy_policy ON legacy_events USING event_at RETAIN FOR '1 day'",
            vec![],
        )
        .unwrap();
    let policy_name = canonical_name("legacy_policy");
    let entries = cassie
        .midge
        .raw_scan_prefix_database("postgres", b"")
        .unwrap();
    let key = entries
        .iter()
        .find(|(_, value)| {
            value
                .windows(policy_name.len())
                .any(|window| window == policy_name.as_bytes())
                && value
                    .windows(b"retention_duration".len())
                    .any(|window| window == b"retention_duration")
        })
        .map(|(key, _)| key.clone())
        .expect("persisted retention metadata key");
    let legacy_json = format!(
        "{{\"name\":\"{policy_name}\",\"collection\":\"postgres.public.legacy_events\",\"timestamp_field\":\"event_at\",\"retention_duration\":\"1 day\",\"enforcement_mode\":\"Explicit\",\"state\":\"Ready\",\"last_enforced_ms\":null,\"last_deleted_rows\":0,\"last_skipped_rows\":0,\"last_error\":null}}"
    );

    // Act
    cassie
        .midge
        .raw_put(
            cassie::midge::adapter::StorageFamily::Data,
            &key,
            legacy_json.as_bytes(),
        )
        .unwrap();
    drop(cassie);
    let recovered = Cassie::new_with_data_dir(&path).unwrap();
    recovered.startup().unwrap();
    let recovered_session = recovered.create_session("tester", None);
    let progress = scheduled_policy_progress(&recovered, &recovered_session, "legacy_policy");

    // Assert
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0][0], Value::String("unscheduled".to_owned()));
    assert_eq!(progress[0][1], Value::Null);
    assert_eq!(progress[0][2], Value::Null);
    assert_eq!(progress[0][3], Value::Int64(0));
    assert_eq!(progress[0][4], Value::Int64(0));
    assert_eq!(progress[0][5], Value::Int64(0));
    assert_eq!(progress[0][6], Value::Null);
    assert_eq!(progress[0][7], Value::String(String::new()));
    assert_eq!(progress[0][8], Value::Null);
    assert_eq!(progress[0][9], Value::Null);
    assert_eq!(progress[0][10], Value::Null);
    assert_eq!(progress[0][11], Value::Null);
    assert_eq!(progress[0][12], Value::String("explicit".to_owned()));
    assert_eq!(progress[0][13], Value::String("ready".to_owned()));
    drop(recovered);
    let _ = std::fs::remove_dir_all(path);
}

fn assert_timestamptz_value(
    row: &[Value],
    value_column: usize,
    text_column: usize,
    expected: &str,
) {
    assert!(matches!(
        row.get(value_column),
        Some(Value::Typed(cassie::types::typed::TypedValue::TimestampTz(
            _
        )))
    ));
    assert_eq!(row[text_column], Value::String(expected.to_owned()));
}

fn assert_explicit_ready_policy(row: &[Value]) {
    assert_eq!(row[12], Value::String("explicit".to_owned()));
    assert_eq!(row[13], Value::String("ready".to_owned()));
}

fn create_scheduled_effect_source(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    policy: &str,
    schedule: bool,
) {
    for sql in [
        format!(
            "CREATE TABLE {table} (id INT PRIMARY KEY, device TEXT NOT NULL, event_at TIMESTAMP NOT NULL, amount INT)"
        ),
        format!("CREATE TABLE {table}_restrict (parent_id INT REFERENCES {table}(id), label TEXT)"),
        format!(
            "CREATE TABLE {table}_cascade (parent_id INT, label TEXT, CONSTRAINT {table}_cascade_fk FOREIGN KEY (parent_id) REFERENCES {table}(id) ON DELETE CASCADE)"
        ),
    ] {
        cassie.execute_sql(session, &sql, vec![]).unwrap();
    }
    for sql in [
        format!("INSERT INTO {table} VALUES (1, 'a', '2026-01-01T01:00:00Z', 1)"),
        format!("INSERT INTO {table} VALUES (2, 'b', '2026-01-01T02:00:00Z', 2)"),
        format!("INSERT INTO {table} VALUES (3, 'a', '2026-01-02T00:00:00Z', 3)"),
        format!("INSERT INTO {table} VALUES (4, 'b', '2026-01-03T00:00:00Z', 4)"),
        format!("INSERT INTO {table}_restrict VALUES (2, 'keep parent')"),
        format!("INSERT INTO {table}_cascade VALUES (1, 'remove with parent')"),
        format!(
            "CREATE INDEX {table}_time ON {table} USING time_series (event_at) WITH (bucket_width = '1 day', partition_by = device)"
        ),
        format!(
            "CREATE ROLLUP {table}_daily ON {table} USING time_bucket('1 day', event_at) GROUP BY device AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum"
        ),
        format!("CREATE RETENTION POLICY {policy} ON {table} USING event_at RETAIN FOR '1 day'"),
    ] {
        cassie.execute_sql(session, &sql, vec![]).unwrap();
    }
    if schedule {
        cassie
            .execute_sql(
                session,
                &format!(
                    "ALTER RETENTION POLICY {policy} SCHEDULE DAILY START AT '2026-01-03T00:00:00Z'"
                ),
                vec![],
            )
            .unwrap();
    }
}

fn retention_source_rows(cassie: &Cassie, session: &CassieSession, table: &str) -> Vec<Vec<Value>> {
    let result = cassie
        .execute_sql(
            session,
            &format!("SELECT id, device, event_at, amount FROM {table} ORDER BY id"),
            vec![],
        )
        .unwrap();
    result.rows().to_vec()
}

fn retention_rollup_rows(cassie: &Cassie, session: &CassieSession, table: &str) -> Vec<Vec<Value>> {
    let result = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(*) AS total, SUM(amount) AS amount_sum FROM {table} GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device"
            ),
            vec![],
        )
        .unwrap();
    result.rows().to_vec()
}

fn retention_rollup_source_rows(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
) -> Vec<Vec<Value>> {
    let result = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT time_bucket('1 day', event_at) AS bucket, device, COUNT(*) AS total, SUM(amount) AS amount_sum FROM {table} WHERE id >= 0 GROUP BY time_bucket('1 day', event_at), device ORDER BY bucket, device"
            ),
            vec![],
        )
        .unwrap();
    result.rows().to_vec()
}

#[test]
fn should_parse_scheduled_retention_commands() {
    // Arrange
    let statements = [
        "ALTER RETENTION POLICY events_retention SCHEDULE DAILY START AT '2026-01-03T00:00:00Z'",
        "ALTER RETENTION POLICY events_retention SUSPEND",
        "ALTER RETENTION POLICY events_retention RESUME",
        "ENFORCE RETENTION POLICY events_retention DUE AT '2026-01-03T00:00:00Z'",
    ];

    // Act
    let parsed = statements.map(parse_statement);

    // Assert
    for result in parsed {
        assert!(
            result.is_ok(),
            "scheduled statement parse failed: {result:?}"
        );
    }
}

#[test]
fn should_enforce_due_window_with_foreign_key_index_and_rollup_parity() {
    // Arrange
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_scheduled_effect_source(
            &cassie,
            &session,
            "scheduled_effect_events",
            "scheduled_effect_policy",
            true,
        );
        create_scheduled_effect_source(
            &cassie,
            &session,
            "manual_effect_events",
            "manual_effect_policy",
            false,
        );

        // Act
        let scheduled = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY scheduled_effect_policy DUE AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let manual = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY manual_effect_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let scheduled_source = retention_source_rows(
            &cassie,
            &session,
            "scheduled_effect_events",
        );
        let manual_source = retention_source_rows(&cassie, &session, "manual_effect_events");
        let scheduled_restrict = cassie
            .execute_sql(
                &session,
                "SELECT parent_id FROM scheduled_effect_events_restrict ORDER BY parent_id",
                vec![],
            )
            .unwrap();
        let manual_restrict = cassie
            .execute_sql(
                &session,
                "SELECT parent_id FROM manual_effect_events_restrict ORDER BY parent_id",
                vec![],
            )
            .unwrap();
        let scheduled_cascade = cassie
            .execute_sql(
                &session,
                "SELECT parent_id FROM scheduled_effect_events_cascade ORDER BY parent_id",
                vec![],
            )
            .unwrap();
        let manual_cascade = cassie
            .execute_sql(
                &session,
                "SELECT parent_id FROM manual_effect_events_cascade ORDER BY parent_id",
                vec![],
            )
            .unwrap();
        let scheduled_index_range = cassie
            .execute_sql(
                &session,
                "SELECT id FROM scheduled_effect_events WHERE device = 'a' AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-02T00:00:00Z' ORDER BY id",
                vec![],
            )
            .unwrap();
        let manual_index_range = cassie
            .execute_sql(
                &session,
                "SELECT id FROM manual_effect_events WHERE device = 'a' AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-02T00:00:00Z' ORDER BY id",
                vec![],
            )
            .unwrap();
        let scheduled_rollup = retention_rollup_rows(
            &cassie,
            &session,
            "scheduled_effect_events",
        );
        let manual_rollup = retention_rollup_rows(&cassie, &session, "manual_effect_events");
        let scheduled_rollup_oracle = retention_rollup_source_rows(
            &cassie,
            &session,
            "scheduled_effect_events",
        );
        let manual_rollup_oracle =
            retention_rollup_source_rows(&cassie, &session, "manual_effect_events");
        let progress = scheduled_policy_progress(
            &cassie,
            &session,
            "scheduled_effect_policy",
        );

        // Assert
        assert_eq!(scheduled.command(), "ENFORCE RETENTION 1");
        assert_eq!(manual.command(), "ENFORCE RETENTION 1");
        assert_eq!(scheduled_source, manual_source);
        assert_eq!(scheduled_source.len(), 3);
        assert_eq!(scheduled_source[0][0], Value::Int64(2));
        assert_eq!(scheduled_source[1][0], Value::Int64(3));
        assert_eq!(scheduled_restrict.rows(), manual_restrict.rows());
        assert_eq!(scheduled_restrict.rows(), vec![vec![Value::Int64(2)]]);
        assert_eq!(scheduled_cascade.rows(), manual_cascade.rows());
        assert!(scheduled_cascade.rows().is_empty());
        assert_eq!(scheduled_index_range.rows(), manual_index_range.rows());
        assert!(scheduled_index_range.rows().is_empty());
        assert_eq!(scheduled_rollup, manual_rollup);
        assert_eq!(scheduled_rollup, scheduled_rollup_oracle);
        assert_eq!(manual_rollup, manual_rollup_oracle);
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0][0], Value::String("active".to_string()));
        assert_explicit_ready_policy(&progress[0]);
        assert_timestamptz_value(&progress[0], 1, 9, "2026-01-04T00:00:00Z");
        assert_eq!(progress[0][2], Value::Null);
        assert_eq!(progress[0][3], Value::Int64(0));
        assert_eq!(progress[0][4], Value::Int64(0));
        assert_eq!(progress[0][5], Value::Int64(0));
        assert_timestamptz_value(&progress[0], 6, 11, "2026-01-03T00:00:00Z");
        assert_eq!(progress[0][7], Value::String(String::new()));
        assert_eq!(progress[0][8], Value::Null);
    });
}

fn create_scheduled_batch_fixture(
    cassie: &Cassie,
    session: &CassieSession,
    table: &str,
    policy: &str,
    expired: usize,
    fresh: usize,
) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {table} (id INT PRIMARY KEY, event_at TIMESTAMP NOT NULL)"),
            vec![],
        )
        .unwrap();
    for id in 1..=expired {
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {table} VALUES ({id}, '2026-01-01T00:00:00Z')"),
                vec![],
            )
            .unwrap();
    }
    for offset in 1..=fresh {
        let id = expired + offset;
        let event_at = if offset == 1 {
            "2026-01-02T00:00:00Z"
        } else {
            "2026-01-10T00:00:00Z"
        };
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {table} VALUES ({id}, '{event_at}')"),
                vec![],
            )
            .unwrap();
    }
    for sql in [
        format!("CREATE RETENTION POLICY {policy} ON {table} USING event_at RETAIN FOR '1 day'"),
        format!("ALTER RETENTION POLICY {policy} SCHEDULE DAILY START AT '2026-01-03T00:00:00Z'"),
    ] {
        cassie.execute_sql(session, &sql, vec![]).unwrap();
    }
}

fn replace_retention_duration(cassie: &Cassie, policy: &str, duration: &str) {
    let mut metadata = cassie
        .catalog
        .get_retention_policy(&canonical_name(policy))
        .expect("retention policy metadata");
    metadata.retention_duration = duration.to_owned();
    cassie.midge.put_retention_policy(&metadata).unwrap();
    cassie.catalog.register_retention_policy(metadata);
}

#[test]
fn should_resume_one_bounded_due_batch_before_later_cutoffs_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("retention_scheduled_resume");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_scheduled_batch_fixture(
            &cassie,
            &session,
            "scheduled_resume_events",
            "scheduled_resume_policy",
            513,
            2,
        );

        // Act
        let first_batch = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY scheduled_resume_policy DUE AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let first_count = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) FROM scheduled_resume_events",
                vec![],
            )
            .unwrap();
        let first_progress =
            scheduled_policy_progress(&cassie, &session, "scheduled_resume_policy");
        cassie
            .execute_sql(
                &session,
                "ALTER RETENTION POLICY scheduled_resume_policy SUSPEND",
                vec![],
            )
            .unwrap();
        let suspended_tick = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY scheduled_resume_policy DUE AT '2026-01-04T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let suspended_progress =
            scheduled_policy_progress(&cassie, &session, "scheduled_resume_policy");
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let same_time_replay = restarted
            .execute_sql(
                &restarted_session,
                "ENFORCE RETENTION POLICY scheduled_resume_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let rows_after_same_replay = restarted
            .execute_sql(
                &restarted_session,
                "SELECT id FROM scheduled_resume_events ORDER BY id",
                vec![],
            )
            .unwrap();
        let same_replay_progress =
            scheduled_policy_progress(&restarted, &restarted_session, "scheduled_resume_policy");
        let different_time_replay = restarted
            .execute_sql(
                &restarted_session,
                "ENFORCE RETENTION POLICY scheduled_resume_policy AT '2026-01-04T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let rows_after_old_window = restarted
            .execute_sql(
                &restarted_session,
                "SELECT id FROM scheduled_resume_events ORDER BY id",
                vec![],
            )
            .unwrap();
        let old_window_progress =
            scheduled_policy_progress(&restarted, &restarted_session, "scheduled_resume_policy");
        restarted
            .execute_sql(
                &restarted_session,
                "ALTER RETENTION POLICY scheduled_resume_policy RESUME",
                vec![],
            )
            .unwrap();
        let second_window = restarted
            .execute_sql(
                &restarted_session,
                "ENFORCE RETENTION POLICY scheduled_resume_policy DUE AT '2026-01-04T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let rows_after_second_window = restarted
            .execute_sql(
                &restarted_session,
                "SELECT id FROM scheduled_resume_events ORDER BY id",
                vec![],
            )
            .unwrap();
        let second_window_progress =
            scheduled_policy_progress(&restarted, &restarted_session, "scheduled_resume_policy");

        // Assert
        assert_eq!(first_batch.command(), "ENFORCE RETENTION 256");
        assert_eq!(first_count.rows(), vec![vec![Value::Int64(259)]]);
        assert_explicit_ready_policy(&first_progress[0]);
        assert_timestamptz_value(&first_progress[0], 2, 10, "2026-01-03T00:00:00Z");
        assert_timestamptz_value(&first_progress[0], 1, 9, "2026-01-03T00:00:00Z");
        assert_eq!(first_progress[0][3], Value::Int64(256));
        assert_eq!(first_progress[0][4], Value::Int64(256));
        assert_eq!(first_progress[0][5], Value::Int64(0));
        assert_eq!(first_progress[0][6], Value::Null);
        assert_eq!(first_progress[0][11], Value::Null);
        assert_eq!(first_progress[0][7], Value::String(String::new()));
        assert_eq!(first_progress[0][8], Value::Null);
        assert_eq!(suspended_tick.command(), "ENFORCE RETENTION 0");
        assert_explicit_ready_policy(&suspended_progress[0]);
        assert_eq!(suspended_progress[0][1], first_progress[0][1]);
        assert_eq!(suspended_progress[0][2], first_progress[0][2]);
        assert_eq!(suspended_progress[0][3], first_progress[0][3]);
        assert_eq!(suspended_progress[0][4], first_progress[0][4]);
        assert_eq!(suspended_progress[0][6], Value::Null);
        assert_eq!(suspended_progress[0][11], Value::Null);
        assert_eq!(same_time_replay.command(), "ENFORCE RETENTION 256");
        assert_eq!(
            rows_after_same_replay.rows(),
            vec![
                vec![Value::Int64(513)],
                vec![Value::Int64(514)],
                vec![Value::Int64(515)]
            ]
        );
        assert_timestamptz_value(&same_replay_progress[0], 2, 10, "2026-01-03T00:00:00Z");
        assert_explicit_ready_policy(&same_replay_progress[0]);
        assert_eq!(same_replay_progress[0][3], Value::Int64(512));
        assert_eq!(same_replay_progress[0][4], Value::Int64(512));
        assert_eq!(same_replay_progress[0][6], Value::Null);
        assert_eq!(same_replay_progress[0][11], Value::Null);
        assert_eq!(different_time_replay.command(), "ENFORCE RETENTION 1");
        assert_eq!(
            rows_after_old_window.rows(),
            vec![vec![Value::Int64(514)], vec![Value::Int64(515)]]
        );
        assert_eq!(old_window_progress[0][2], Value::Null);
        assert_explicit_ready_policy(&old_window_progress[0]);
        assert_timestamptz_value(&old_window_progress[0], 1, 9, "2026-01-04T00:00:00Z");
        assert_timestamptz_value(&old_window_progress[0], 6, 11, "2026-01-03T00:00:00Z");
        assert_eq!(second_window.command(), "ENFORCE RETENTION 1");
        assert_explicit_ready_policy(&second_window_progress[0]);
        assert_eq!(
            rows_after_second_window.rows(),
            vec![vec![Value::Int64(259)]]
        );
        assert_timestamptz_value(&second_window_progress[0], 6, 11, "2026-01-04T00:00:00Z");
        assert_timestamptz_value(&second_window_progress[0], 1, 9, "2026-01-05T00:00:00Z");

        drop(restarted);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_last_scheduled_failure_across_successful_replay() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_failure_events",
        "scheduled_failure_policy",
        257,
        0,
    );

    // Act
    let first_batch = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap();
    replace_retention_duration(&cassie, "scheduled_failure_policy", "1 fortnight");
    let first_failure = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap_err()
        .to_string();
    let failure_progress = scheduled_policy_progress(&cassie, &session, "scheduled_failure_policy");
    cassie
        .execute_sql(
            &session,
            "ALTER RETENTION POLICY scheduled_failure_policy RETAIN FOR '1 day'",
            vec![],
        )
        .unwrap();
    let resumed = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy AT '2026-01-03T00:00:00Z'",
            vec![],
        )
        .unwrap();
    let success_progress = scheduled_policy_progress(&cassie, &session, "scheduled_failure_policy");
    replace_retention_duration(&cassie, "scheduled_failure_policy", "invalid duration");
    let second_failure = cassie
        .execute_sql(
            &session,
            "ENFORCE RETENTION POLICY scheduled_failure_policy DUE AT '2026-01-04T00:00:00Z'",
            vec![],
        )
        .unwrap_err()
        .to_string();
    let latest_failure_progress =
        scheduled_policy_progress(&cassie, &session, "scheduled_failure_policy");

    // Assert
    assert_eq!(first_batch.command(), "ENFORCE RETENTION 256");
    assert_eq!(failure_progress[0][7], Value::String(first_failure.clone()));
    assert_eq!(failure_progress[0][8], Value::String(first_failure.clone()));
    assert_eq!(failure_progress[0][13], Value::String("error".to_owned()));
    assert_timestamptz_value(&failure_progress[0], 2, 10, "2026-01-03T00:00:00Z");
    assert_eq!(resumed.command(), "ENFORCE RETENTION 1");
    assert_eq!(success_progress[0][7], Value::String(String::new()));
    assert_eq!(success_progress[0][8], Value::String(first_failure));
    assert_eq!(success_progress[0][13], Value::String("ready".to_owned()));
    assert_timestamptz_value(&success_progress[0], 6, 11, "2026-01-03T00:00:00Z");
    assert_ne!(
        Value::String(second_failure.clone()),
        success_progress[0][8]
    );
    assert_eq!(
        latest_failure_progress[0][7],
        Value::String(second_failure.clone())
    );
    assert_eq!(latest_failure_progress[0][8], Value::String(second_failure));
    assert_eq!(
        latest_failure_progress[0][13],
        Value::String("error".to_owned())
    );
    assert_timestamptz_value(&latest_failure_progress[0], 6, 11, "2026-01-03T00:00:00Z");
    assert_timestamptz_value(&latest_failure_progress[0], 1, 9, "2026-01-04T00:00:00Z");
}

#[test]
fn should_advance_scheduled_cursor_when_a_bounded_batch_deletes_no_rows() {
    // Arrange
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_scheduled_batch_fixture(
            &cassie,
            &session,
            "scheduled_cursor_events",
            "scheduled_cursor_policy",
            0,
            257,
        );
        // Act
        let metadata_only = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY scheduled_cursor_policy DUE AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let first_progress =
            scheduled_policy_progress(&cassie, &session, "scheduled_cursor_policy");
        cassie
            .execute_sql(
                &session,
                "UPDATE scheduled_cursor_events SET event_at = '2026-01-01T00:00:00Z' WHERE id = 1",
                vec![],
            )
            .unwrap();
        let finish_pending_window = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY scheduled_cursor_policy DUE AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let completed_first_progress =
            scheduled_policy_progress(&cassie, &session, "scheduled_cursor_policy");
        let remaining_before_correction_window = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) FROM scheduled_cursor_events",
                vec![],
            )
            .unwrap();
        let next_window = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY scheduled_cursor_policy DUE AT '2026-01-04T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let completed_second_progress =
            scheduled_policy_progress(&cassie, &session, "scheduled_cursor_policy");
        let remaining = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) FROM scheduled_cursor_events",
                vec![],
            )
            .unwrap();
        let corrected_row = cassie
            .execute_sql(
                &session,
                "SELECT id FROM scheduled_cursor_events WHERE id = 1",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(metadata_only.command(), "ENFORCE RETENTION 0");
        assert_explicit_ready_policy(&first_progress[0]);
        assert_timestamptz_value(&first_progress[0], 2, 10, "2026-01-03T00:00:00Z");
        assert_timestamptz_value(&first_progress[0], 1, 9, "2026-01-03T00:00:00Z");
        assert_eq!(first_progress[0][3], Value::Int64(256));
        assert_eq!(first_progress[0][4], Value::Int64(0));
        assert_eq!(first_progress[0][5], Value::Int64(0));
        assert_eq!(first_progress[0][6], Value::Null);
        assert_eq!(first_progress[0][11], Value::Null);
        assert_eq!(finish_pending_window.command(), "ENFORCE RETENTION 0");
        assert_explicit_ready_policy(&completed_first_progress[0]);
        assert_timestamptz_value(&completed_first_progress[0], 1, 9, "2026-01-04T00:00:00Z");
        assert_eq!(completed_first_progress[0][2], Value::Null);
        assert_timestamptz_value(&completed_first_progress[0], 6, 11, "2026-01-03T00:00:00Z");
        assert_eq!(
            remaining_before_correction_window.rows(),
            vec![vec![Value::Int64(257)]]
        );
        assert_eq!(next_window.command(), "ENFORCE RETENTION 1");
        assert_explicit_ready_policy(&completed_second_progress[0]);
        assert_timestamptz_value(&completed_second_progress[0], 1, 9, "2026-01-05T00:00:00Z");
        assert_timestamptz_value(&completed_second_progress[0], 6, 11, "2026-01-04T00:00:00Z");
        assert_eq!(remaining.rows(), vec![vec![Value::Int64(256)]]);
        assert!(corrected_row.rows().is_empty());
    });
}

#[test]
fn should_serialize_overlapping_duplicate_due_ticks_without_double_deleting() {
    // Arrange
    let cassie = std::sync::Arc::new(crate::support_sql::new_in_memory_cassie().unwrap());
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    create_scheduled_batch_fixture(
        &cassie,
        &session,
        "scheduled_overlap_events",
        "scheduled_overlap_policy",
        1,
        0,
    );
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let cassie = std::sync::Arc::clone(&cassie);
        let barrier = std::sync::Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            let runtime = runtime();
            let session = cassie.create_session("scheduled-worker", None);
            barrier.wait();
            runtime.block_on(async {
                cassie
                    .execute_sql(
                        &session,
                        "ENFORCE RETENTION POLICY scheduled_overlap_policy DUE AT '2026-01-03T00:00:00Z'",
                        vec![],
                    )
                    .expect("concurrent due tick")
                    .command()
                    .to_owned()
            })
        }));
    }

    // Act
    barrier.wait();
    let mut commands = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker completed"))
        .collect::<Vec<_>>();
    commands.sort();
    let remaining = cassie
        .execute_sql(
            &session,
            "SELECT COUNT(*) FROM scheduled_overlap_events",
            vec![],
        )
        .unwrap();

    // Assert
    assert_eq!(commands, vec!["ENFORCE RETENTION 0", "ENFORCE RETENTION 1"]);
    assert_eq!(remaining.rows(), vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_deny_scheduled_retention_commands_to_authenticated_non_admin_roles() {
    // Arrange
    runtime().block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let admin = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &admin,
                "CREATE TABLE scheduled_privilege_events (event_at TIMESTAMP NOT NULL)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &admin,
                "CREATE RETENTION POLICY scheduled_privilege_policy ON scheduled_privilege_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();
        let password = crate::support_sql::test_root_password();
        cassie
            .execute_sql(
                &admin,
                &format!(
                    "CREATE ROLE retention_operator LOGIN PASSWORD '{password}'"
                ),
                vec![],
            )
            .unwrap();
        let operator = cassie
            .authenticate_role(
                "retention_operator",
                Some(password),
                Some("postgres".to_owned()),
            )
            .expect("authenticate non-admin role");

        // Act
        let schedule = cassie.execute_sql(
            &operator,
            "ALTER RETENTION POLICY scheduled_privilege_policy SCHEDULE DAILY START AT '2026-01-03T00:00:00Z'",
            vec![],
        );
        let suspend = cassie.execute_sql(
            &operator,
            "ALTER RETENTION POLICY scheduled_privilege_policy SUSPEND",
            vec![],
        );
        let due = cassie.execute_sql(
            &operator,
            "ENFORCE RETENTION POLICY scheduled_privilege_policy DUE AT '2026-01-03T00:00:00Z'",
            vec![],
        );

        // Assert
        assert_eq!(schedule.unwrap_err().sql_state(), "42501");
        assert_eq!(suspend.unwrap_err().sql_state(), "42501");
        assert_eq!(due.unwrap_err().sql_state(), "42501");
    });
}
