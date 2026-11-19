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
                "SELECT schedule_state, next_due_at, pending_cutoff, pending_scanned_rows, pending_deleted_rows, pending_skipped_rows, last_successful_cutoff, last_error, last_failure, next_due_at::TEXT, pending_cutoff::TEXT, last_successful_cutoff::TEXT, enforcement_mode, state, last_deleted_rows, last_skipped_rows FROM pg_catalog.pg_retention_policies WHERE policy_name = '{policy_name}'"
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

/// Checks a `TIMESTAMPTZ` progress column and its `::TEXT` cast against the
/// expected UTC instant, both rendered in PostgreSQL text form.
fn assert_timestamptz_value(
    row: &[Value],
    value_column: usize,
    text_column: usize,
    expected: &str,
) {
    let rendered = expected.replace('T', " ").replace('Z', "+00");
    assert_eq!(row[value_column], Value::String(rendered.clone()));
    assert_eq!(row[text_column], Value::String(rendered));
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

include!("scheduled_recovery.rs");
