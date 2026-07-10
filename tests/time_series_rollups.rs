use cassie::app::Cassie;
use cassie::catalog::{canonical_relation_name, RollupState};
use cassie::types::Value;

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
fn should_refresh_rollup_for_dml_movement() {
    // Arrange
    with_fallback();
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
