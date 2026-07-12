use cassie::app::Cassie;
use cassie::executor::set_materialized_projection_maintenance_failure_point;
use cassie::midge::adapter::set_rollup_maintenance_failure_point;
use cassie::types::Value;

static MATERIALIZED_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());
static ROLLUP_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[path = "support/sql.rs"]
mod support;
use support::*;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[test]
fn should_not_retry_a_durable_commit_after_materialized_refresh_failure() {
    // Arrange
    with_fallback();
    let _failpoint_guard = MATERIALIZED_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("transaction_commit_materialized_boundary");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_materialized_source (tenant TEXT, amount INT)",
                vec![],
            )
            .expect("create source");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_materialized_source (tenant, amount) VALUES ('acme', 10)",
                vec![],
            )
            .expect("seed source");
        cassie
            .execute_sql(
                &session,
                "CREATE MATERIALIZED PROJECTION transaction_materialized WITH (analytical = true) AS SELECT tenant, amount FROM transaction_materialized_source",
                vec![],
            )
            .expect("create projection");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_materialized_source (tenant, amount) VALUES ('acme', 20)",
                vec![],
            )
            .expect("stage write");
        set_materialized_projection_maintenance_failure_point(true);

        // Act
        let commit = cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("base commit remains successful");
        let retry = cassie.execute_sql(&session, "COMMIT", vec![]);
        let rollback = cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("rollback after a durable commit remains harmless");
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM transaction_materialized_source ORDER BY amount",
                vec![],
            )
            .expect("read committed source");
        let debt = cassie
            .execute_sql(
                &session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.transaction_materialized_source'",
                vec![],
            )
            .expect("read maintenance debt");

        // Assert
        assert_eq!(commit.command, "COMMIT");
        assert!(retry.is_err(), "a durable COMMIT must not be retryable");
        assert_eq!(rollback.command, "ROLLBACK");
        assert_eq!(session.transaction_status(), "idle");
        assert_eq!(
            rows.rows,
            vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]
        );
        assert_eq!(
            debt.rows,
            vec![vec![Value::String("materialized_projection".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_replay_rollup_debt_after_durable_transaction_commit() {
    // Arrange
    with_fallback();
    let _failpoint_guard = ROLLUP_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("transaction_commit_rollup_boundary");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE transaction_rollup_source (tenant TEXT, event_at TEXT, amount INT)",
                vec![],
            )
            .expect("create source");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_rollup_source (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:05:00Z', 10)",
                vec![],
            )
            .expect("seed source");
        cassie
            .execute_sql(
                &session,
                "CREATE ROLLUP transaction_rollup ON transaction_rollup_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
                vec![],
            )
            .expect("create rollup");
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO transaction_rollup_source (tenant, event_at, amount) VALUES ('acme', '2026-01-01T00:25:00Z', 20)",
                vec![],
            )
            .expect("stage write");
        set_rollup_maintenance_failure_point(true);

        // Act
        let commit = cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("base commit remains successful");
        let retry = cassie.execute_sql(&session, "COMMIT", vec![]);
        let source_rows = cassie
            .execute_sql(
                &session,
                "SELECT amount FROM transaction_rollup_source ORDER BY amount",
                vec![],
            )
            .expect("read committed source");
        let debt = cassie
            .execute_sql(
                &session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.transaction_rollup_source'",
                vec![],
            )
            .expect("read maintenance debt");
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("restart cassie");
        restarted.startup().expect("restart startup");
        let restarted_session = restarted.create_session("tester", None);
        let rollup_rows = restarted
            .execute_sql(
                &restarted_session,
                "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) AS amount_sum FROM transaction_rollup_source GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY bucket, tenant",
                vec![],
            )
            .expect("read recovered rollup");
        let remaining_debt = restarted
            .execute_sql(
                &restarted_session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.transaction_rollup_source'",
                vec![],
            )
            .expect("read recovered maintenance debt");

        // Assert
        assert_eq!(commit.command, "COMMIT");
        assert!(retry.is_err(), "a durable COMMIT must not be retryable");
        assert_eq!(session.transaction_status(), "idle");
        assert_eq!(
            source_rows.rows,
            vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]
        );
        assert_eq!(
            debt.rows,
            vec![vec![Value::String("rollup".to_string())]]
        );
        assert_eq!(
            rollup_rows.rows,
            vec![vec![
                Value::String("2026-01-01T00:00:00Z".to_string()),
                Value::String("acme".to_string()),
                Value::Int64(2),
                Value::Int64(30),
            ]]
        );
        assert!(remaining_debt.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}
