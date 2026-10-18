use cassie::app::Cassie;
use cassie::catalog::{canonical_relation_name, output_collection_name, RollupState};
use cassie::midge::adapter::set_rollup_maintenance_failure_point;
use cassie::types::Value;

static ROLLUP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

use super::support_sql as support;
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

fn simulate_interrupted_rollup_refresh(cassie: &Cassie, name: &str) {
    let mut interrupted = cassie
        .catalog
        .get_rollup(&canonical_name(name))
        .expect("rollup metadata");
    interrupted.state = RollupState::Building;
    cassie
        .midge
        .put_rollup(&interrupted)
        .expect("persist interrupted refresh state");
    cassie.catalog.register_rollup(interrupted.clone());
    cassie
        .midge
        .drop_collection(&interrupted.output_collection)
        .expect("remove output after replacement begins");
}

fn create_rollup_publication_fixture(path: &str) -> (std::sync::Arc<Cassie>, String, String, u64) {
    let cassie = std::sync::Arc::new(Cassie::new_with_data_dir(path).unwrap());
    cassie.startup().unwrap();
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE rollup_publication_events (tenant TEXT, event_at TEXT, amount INT)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO rollup_publication_events VALUES ('a', '2026-01-01T00:05:00Z', 1)",
            vec![],
        )
        .unwrap();
    cassie
            .execute_sql(
                &setup,
                "CREATE ROLLUP rollup_publication_hourly ON rollup_publication_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
                vec![],
            )
            .unwrap();
    let source = canonical_name("rollup_publication_events");
    let rollup = canonical_name("rollup_publication_hourly");
    let generation = cassie.midge.collection_generation(&source).unwrap();
    (cassie, source, rollup, generation)
}

include!("semantics.rs");

include!("restart.rs");

include!("errors.rs");

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
    assert_eq!(inserted.command(), "INSERT 0 1");
}
