use cassie::app::Cassie;
use cassie::sql::ast::QueryStatement;
use cassie::sql::parse_statement;
use cassie::types::Value;

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
fn should_parse_retention_policy_commands() {
    // Arrange
    let create = "CREATE RETENTION POLICY IF NOT EXISTS events_retention ON events USING event_at RETAIN FOR '7 days'";
    let alter = "ALTER RETENTION POLICY events_retention RETAIN FOR '2 days'";
    let enforce = "ENFORCE RETENTION POLICY events_retention AT '2026-01-10T00:00:00Z'";
    let drop = "DROP RETENTION POLICY IF EXISTS events_retention";

    // Act
    let create = parse_statement(create).expect("create retention policy parses");
    let alter = parse_statement(alter).expect("alter retention policy parses");
    let enforce = parse_statement(enforce).expect("enforce retention policy parses");
    let drop = parse_statement(drop).expect("drop retention policy parses");

    // Assert
    assert!(matches!(
        create.statement,
        QueryStatement::CreateRetentionPolicy(_)
    ));
    assert!(matches!(
        alter.statement,
        QueryStatement::AlterRetentionPolicy(_)
    ));
    assert!(matches!(
        enforce.statement,
        QueryStatement::EnforceRetentionPolicy(_)
    ));
    assert!(matches!(
        drop.statement,
        QueryStatement::DropRetentionPolicy(_)
    ));
}

#[test]
fn should_lifecycle_retention_policy_metadata() {
    // Arrange
    with_fallback();
    let path = data_dir("retention_catalog");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE retention_catalog_events (event_at TEXT, kind TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY retention_catalog_policy ON retention_catalog_events USING event_at RETAIN FOR '7 days'",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ALTER RETENTION POLICY retention_catalog_policy RETAIN FOR '2 days'",
                vec![],
            )
            .unwrap();
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        let policies = restarted
            .execute_sql(
                &restarted_session,
                "SELECT policy_name, retention_duration, state FROM pg_catalog.pg_retention_policies WHERE policy_name = 'retention_catalog_policy'",
                vec![],
            )
            .unwrap();
        restarted
            .execute_sql(
                &restarted_session,
                "DROP RETENTION POLICY retention_catalog_policy",
                vec![],
            )
            .unwrap();
        let dropped = restarted
            .execute_sql(
                &restarted_session,
                "SELECT policy_name FROM pg_catalog.pg_retention_policies WHERE policy_name = 'retention_catalog_policy'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            policies.rows,
            vec![vec![
                Value::String("retention_catalog_policy".to_string()),
                Value::String("2 days".to_string()),
                Value::String("ready".to_string()),
            ]]
        );
        assert!(dropped.rows.is_empty());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_enforce_retention_idempotently() {
    // Arrange
    with_fallback();
    let path = data_dir("retention_enforce");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE retention_enforce_events (event_at TEXT, kind TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX retention_enforce_kind_idx ON retention_enforce_events (kind)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES ('2026-01-01T00:00:00Z', 'old')",
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES ('2026-01-02T12:00:00Z', 'fresh')",
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES ('not-a-time', 'bad')",
            "INSERT INTO retention_enforce_events (event_at, kind) VALUES (NULL, 'missing')",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY retention_enforce_policy ON retention_enforce_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();

        // Act
        let first = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY retention_enforce_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let second = cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY retention_enforce_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT kind FROM retention_enforce_events ORDER BY kind",
                vec![],
            )
            .unwrap();
        let indexed = cassie
            .execute_sql(
                &session,
                "SELECT kind FROM retention_enforce_events WHERE kind = 'old'",
                vec![],
            )
            .unwrap();
        let metrics = cassie.metrics();
        let policies = cassie
            .execute_sql(
                &session,
                "SELECT last_deleted_rows, last_skipped_rows FROM pg_catalog.pg_retention_policies WHERE policy_name = 'retention_enforce_policy'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(first.command, "ENFORCE RETENTION 1");
        assert_eq!(second.command, "ENFORCE RETENTION 0");
        assert_eq!(
            rows.rows,
            vec![
                vec![Value::String("bad".to_string())],
                vec![Value::String("fresh".to_string())],
                vec![Value::String("missing".to_string())],
            ]
        );
        assert!(indexed.rows.is_empty());
        assert_eq!(metrics["retention"]["enforcements"].as_u64(), Some(2));
        assert_eq!(metrics["retention"]["deleted_rows"].as_u64(), Some(1));
        assert_eq!(metrics["retention"]["skipped_rows"].as_u64(), Some(4));
        assert_eq!(
            policies.rows,
            vec![vec![Value::Int64(0), Value::Int64(2)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_refresh_rollup_after_retention_enforcement() {
    // Arrange
    with_fallback();
    let path = data_dir("retention_rollup");

    runtime().block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE retention_rollup_events (tenant TEXT, event_at TEXT, amount INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO retention_rollup_events (tenant, event_at, amount) VALUES ('a', '2026-01-01T00:05:00Z', 7)",
            "INSERT INTO retention_rollup_events (tenant, event_at, amount) VALUES ('a', '2026-01-02T12:00:00Z', 5)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE ROLLUP retention_rollup_hourly ON retention_rollup_events USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE RETENTION POLICY retention_rollup_policy ON retention_rollup_events USING event_at RETAIN FOR '1 day'",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "ENFORCE RETENTION POLICY retention_rollup_policy AT '2026-01-03T00:00:00Z'",
                vec![],
            )
            .unwrap();
        let rollup = cassie
            .execute_sql(
                &session,
                "SELECT time_bucket('1 hour', event_at) AS bucket, tenant, COUNT(*) AS total, SUM(amount) AS amount_sum FROM retention_rollup_events GROUP BY time_bucket('1 hour', event_at), tenant ORDER BY bucket, tenant",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            rollup.rows,
            vec![vec![
                Value::String("2026-01-02T12:00:00Z".to_string()),
                Value::String("a".to_string()),
                Value::Int64(1),
                Value::Int64(5),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
