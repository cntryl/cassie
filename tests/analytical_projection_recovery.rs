use cassie::app::Cassie;
use cassie::app::CassieSession;
use cassie::executor::set_materialized_projection_maintenance_failure_point;
use cassie::types::Value;
use std::path::Path;

static MATERIALIZED_PROJECTION_FAILPOINT_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[path = "support/sql.rs"]
mod support;
use support::*;

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn setup_source_and_projection(path: &Path) -> (Cassie, CassieSession) {
    let cassie = Cassie::new_with_data_dir(path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE analytical_debt_source (tenant TEXT, amount INT)",
            vec![],
        )
        .expect("create source");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO analytical_debt_source (tenant, amount) VALUES ('acme', 10)",
            vec![],
        )
        .expect("seed source");
    cassie
        .execute_sql(
            &session,
            "CREATE MATERIALIZED PROJECTION analytical_debt WITH (analytical = true) AS SELECT tenant, amount FROM analytical_debt_source",
            vec![],
        )
        .expect("create projection");
    (cassie, session)
}

#[test]
fn should_replay_materialized_projection_debt_after_post_commit_stale_failure() {
    // Arrange
    with_fallback();
    let _failpoint_guard = MATERIALIZED_PROJECTION_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("analytical_projection_maintenance_debt");

    runtime().block_on(async {
        let (cassie, session) = setup_source_and_projection(path.as_ref());
        // Act
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        let insert = cassie
            .execute_sql(
                &session,
                "INSERT INTO analytical_debt_source (tenant, amount) VALUES ('acme', 20)",
                vec![],
            )
            .expect("stage write");
        set_materialized_projection_maintenance_failure_point(true);
        let commit = cassie
            .execute_sql(&session, "COMMIT", vec![])
            .expect("base commit remains successful");
        let debt = cassie
            .execute_sql(
                &session,
                "SELECT artifact, target_generation, retry_count, last_error, fallback_reason FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.analytical_debt_source'",
                vec![],
            )
            .expect("read maintenance debt");
        let before_restart = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM analytical_debt_source ORDER BY amount",
                vec![],
            )
            .expect("read source fallback");
        let before_restart_metrics = cassie.metrics();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("restart cassie");
        restarted.startup().expect("restart startup");
        let restarted_session = restarted.create_session("tester", None);
        let restarted_debt = restarted
            .execute_sql(
                &restarted_session,
                "SELECT artifact FROM pg_catalog.pg_maintenance_debt WHERE collection = 'postgres.public.analytical_debt_source'",
                vec![],
            )
            .expect("read replayed debt");
        let projection = restarted
            .execute_sql(
                &restarted_session,
                "SELECT state FROM pg_catalog.pg_materialized_projections WHERE projection_name = 'postgres.public.analytical_debt'",
                vec![],
            )
            .expect("read projection state");
        let after_restart = restarted
            .execute_sql(
                &restarted_session,
                "SELECT tenant, amount FROM analytical_debt_source ORDER BY amount",
                vec![],
            )
            .expect("read source after restart");
        let after_restart_metrics = restarted.metrics();

        // Assert
        assert_eq!(insert.command, "INSERT 0 1");
        assert_eq!(commit.command, "COMMIT");
        assert_eq!(before_restart.rows.len(), 2);
        assert_eq!(
            before_restart_metrics["projections"]["last_fallback_reason"].as_str(),
            Some("maintenance_pending")
        );
        assert_eq!(debt.rows.len(), 1);
        assert_eq!(
            debt.rows[0],
            vec![
                Value::String("materialized_projection".to_string()),
                Value::Int64(2),
                Value::Int64(1),
                Value::String(
                    "materialized_projection maintenance failed (details redacted)".to_string(),
                ),
                Value::String("maintenance_pending".to_string()),
            ]
        );
        assert!(restarted_debt.rows.is_empty());
        assert_eq!(
            projection.rows,
            vec![vec![Value::String("stale".to_string())]]
        );
        assert_eq!(after_restart.rows.len(), 2);
        assert_eq!(
            after_restart_metrics["projections"]["last_fallback_reason"].as_str(),
            Some("stale-or-unverified")
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_analytical_projection_with_mismatched_source_generation() {
    // Arrange
    with_fallback();
    let path = data_dir("analytical_projection_generation_fence");

    runtime().block_on(async {
        let (cassie, session) = setup_source_and_projection(path.as_ref());
        let mut projection = cassie
            .catalog
            .get_materialized_projection("analytical_debt")
            .expect("projection metadata");
        projection
            .source_generations
            .insert("postgres.public.analytical_debt_source".to_string(), 0);
        cassie.midge.put_projection_metadata(&projection).unwrap();
        cassie.catalog.register_projection_metadata(projection);

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT tenant, amount FROM analytical_debt_source ORDER BY amount",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][1], Value::Int64(10));

        let _ = std::fs::remove_dir_all(path);
    });
}
