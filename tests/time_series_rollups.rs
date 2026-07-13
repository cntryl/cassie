use cassie::app::Cassie;
use cassie::catalog::{canonical_relation_name, RollupState};
use cassie::midge::adapter::set_rollup_maintenance_failure_point;
use cassie::types::Value;

static ROLLUP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[path = "support/sql.rs"]
mod support;
use support::*;

fn canonical_name(name: &str) -> String {
    canonical_relation_name("postgres", "public", name)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn seed_events(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {table} (tenant TEXT, event_at TEXT, amount INT)"),
            vec![],
        )
        .unwrap();
    for sql in [
        format!(
            "INSERT INTO {table} (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:05:00Z', 7)"
        ),
        format!(
            "INSERT INTO {table} (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:25:00Z', 5)"
        ),
        format!(
            "INSERT INTO {table} (tenant, event_at, amount) VALUES ('b', '2026-01-01T01:05:00Z', 3)"
        ),
    ] {
        cassie.execute_sql(session, &sql, vec![]).unwrap();
    }
}

fn create_hourly_rollup(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
    cassie
        .execute_sql(
            session,
            &format!(
                "CREATE ROLLUP {table}_hourly ON {table} USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum"
            ),
            vec![],
        )
        .unwrap();
}

fn hourly_query(table: &str) -> String {
    format!(
        "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) AS amount_sum FROM {table} GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY bucket, tenant"
    )
}

#[test]
fn should_rewrite_query_after_rollup_creation() {
    // Arrange
    with_fallback();
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_rewrite");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
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
            selected.rows,
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
            &explain.rows[0][0],
            Value::String(plan) if plan.contains(&format!("rollup_rewrite={rollup_name}"))
        ));
        assert!(cassie.catalog.get_rollup(&rollup_name).is_some());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_rollup_with_mismatched_source_generation() {
    // Arrange
    with_fallback();
    let path = data_dir("rollup_generation_fence");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
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
        assert_eq!(result.rows.len(), 2);
        assert_eq!(metrics["rollups"]["rewrite_hits"].as_u64(), Some(0));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_refresh_rollup_for_dml_movement() {
    // Arrange
    with_fallback();
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_dml");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
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
            selected.rows,
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

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_cleanup_rollup_after_restart_drop() {
    // Arrange
    with_fallback();
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
            catalog_rows.rows,
            vec![vec![
                Value::String(rollup_name.clone()),
                Value::String("ready".to_string())
            ]]
        );
        assert!(restarted.catalog.get_rollup(&rollup_name).is_none());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fallback_to_source_when_rollup_is_stale() {
    // Arrange
    with_fallback();
    let _rollup_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_stale");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
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
        assert_eq!(selected.rows.len(), 2);
        assert_eq!(metrics["rollups"]["stale_fallbacks"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}

fn inject_rollup_refresh_failure(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    table: &str,
) {
    set_rollup_maintenance_failure_point(true);
    let inserted = cassie
        .execute_sql(
            session,
            &format!(
                "INSERT INTO {table} (tenant, event_at, amount) VALUES ('a', '2026-01-01T01:30:00Z', 11)"
            ),
            vec![],
        )
        .unwrap();
    assert_eq!(inserted.command, "INSERT 0 1");
}

#[test]
fn should_record_rollup_debt_after_refresh_failure() {
    // Arrange
    with_fallback();
    let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_maintenance_debt");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
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
        assert_eq!(source_rows.rows.len(), 3, "{:?}", source_rows.rows);
        assert_eq!(source_rows.rows[0][2], Value::Int64(2));
        assert_eq!(source_rows.rows[0][3], Value::Int64(12));
        assert_eq!(debt.rows.len(), 1);
        assert_eq!(
            debt.rows[0],
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

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_retry_rollup_debt_on_startup() {
    // Arrange
    with_fallback();
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
        assert_eq!(recovered.rows.len(), 3);
        assert_eq!(recovered.rows[0][2], Value::Int64(2));
        assert_eq!(recovered.rows[0][3], Value::Int64(12));
        assert!(remaining_debt.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_move_rollup_debt_with_collection_rename() {
    // Arrange
    with_fallback();
    let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("rollup_rename_debt");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
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

        let _ = std::fs::remove_dir_all(path);
    });
}
