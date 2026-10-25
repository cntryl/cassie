//! PostgreSQL 18.6 transactional catalog publication tracked by #833.

use super::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::Value;

const MATERIALIZED_VIEW_NAME: &str = "issue833_transactional_materialized_view";

#[path = "alter_rollback_visibility.rs"]
mod alter_rollback_visibility;

#[path = "catalog_snapshot.rs"]
mod catalog_snapshot;

#[path = "restart_publication.rs"]
mod restart_publication;

fn visible_materialized_view(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT matviewname FROM pg_catalog.pg_matviews \
             WHERE schemaname = 'public' \
               AND matviewname = 'issue833_transactional_materialized_view'",
            vec![],
        )
        .expect("read the materialized view catalog")
        .rows()
        .to_vec()
}

#[test]
fn should_restore_transactional_catalog_at_savepoint_rollback() {
    // Arrange
    let fixture = sql_fixture("issue-833-ddl-savepoint", &[]);
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("CREATE TABLE tx_ddl_savepoint_kept (id INT PRIMARY KEY)")
        .expect("create table before savepoint");
    fixture
        .execute("SAVEPOINT before_second_ddl")
        .expect("create savepoint");
    fixture
        .execute("CREATE TABLE tx_ddl_savepoint_rolled_back (id INT PRIMARY KEY)")
        .expect("create table after savepoint");

    // Act
    fixture
        .execute("ROLLBACK TO SAVEPOINT before_second_ddl")
        .expect("roll back catalog changes after savepoint");
    fixture
        .execute("COMMIT")
        .expect("commit recovered transaction");
    let visible_tables = fixture.rows(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' \
           AND table_name LIKE 'tx_ddl_savepoint_%' \
         ORDER BY table_name",
    );

    // Assert
    assert_eq!(
        visible_tables,
        vec![vec![Value::String("tx_ddl_savepoint_kept".to_owned())]]
    );
}

#[test]
fn should_restore_dropped_table_after_transaction_rollback() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-drop-rollback",
        &[
            "CREATE TABLE tx_ddl_drop_restore (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO tx_ddl_drop_restore VALUES (1, 'retained')",
        ],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("DROP TABLE tx_ddl_drop_restore")
        .expect("drop table transactionally");

    // Act
    fixture.execute("ROLLBACK").expect("roll back table drop");
    let rows = fixture.rows("SELECT id, label FROM tx_ddl_drop_restore ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Int64(1), Value::String("retained".to_owned())]]
    );
}

#[test]
fn should_commit_table_drop_after_staging_a_row_write() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-drop-after-staged-write",
        &["CREATE TABLE tx_ddl_drop_staged (id INT PRIMARY KEY)"],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("INSERT INTO tx_ddl_drop_staged VALUES (1)")
        .expect("stage a row write");
    fixture
        .execute("DROP TABLE tx_ddl_drop_staged")
        .expect("drop table transactionally");

    // Act
    fixture
        .execute("COMMIT")
        .expect("commit the staged write and table drop");
    let absent = fixture.execute("SELECT id FROM tx_ddl_drop_staged");

    // Assert
    assert!(
        absent.is_err(),
        "dropped table must stay absent after commit"
    );
}

#[test]
fn should_publish_a_staged_row_after_transactional_table_rename() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-rename-after-staged-write",
        &[
            "CREATE TABLE tx_ddl_rename_staged (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO tx_ddl_rename_staged VALUES (1, 'existing')",
        ],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("INSERT INTO tx_ddl_rename_staged VALUES (2, 'staged')")
        .expect("stage a row write");
    fixture
        .execute("ALTER TABLE tx_ddl_rename_staged RENAME TO tx_ddl_renamed")
        .expect("rename table transactionally");

    // Act
    fixture
        .execute("COMMIT")
        .expect("commit the staged row and table rename");
    let rows = fixture.rows("SELECT id, label FROM tx_ddl_renamed ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(1), Value::String("existing".to_owned())],
            vec![Value::Int64(2), Value::String("staged".to_owned())],
        ]
    );
}

#[test]
fn should_publish_a_staged_row_after_transactional_schema_rename() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-schema-rename-after-staged-write",
        &[
            "CREATE SCHEMA tx_ddl_schema_before",
            "CREATE TABLE tx_ddl_schema_before.tx_ddl_schema_rows (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO tx_ddl_schema_before.tx_ddl_schema_rows VALUES (1, 'existing')",
        ],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("INSERT INTO tx_ddl_schema_before.tx_ddl_schema_rows VALUES (2, 'staged')")
        .expect("stage a row write");
    fixture
        .execute("ALTER SCHEMA tx_ddl_schema_before RENAME TO tx_ddl_schema_after")
        .expect("rename schema transactionally");
    fixture
        .execute("SELECT id FROM tx_ddl_schema_after.tx_ddl_schema_rows")
        .expect("read the table from the renamed schema before commit");

    // Act
    fixture
        .execute("COMMIT")
        .expect("commit the staged row and schema rename");
    let rows = fixture
        .execute("SELECT id, label FROM tx_ddl_schema_after.tx_ddl_schema_rows ORDER BY id")
        .expect("read table after the schema rename")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(1), Value::String("existing".to_owned())],
            vec![Value::Int64(2), Value::String("staged".to_owned())],
        ]
    );
}

#[test]
fn should_publish_a_view_after_transactional_schema_rename() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-schema-rename-with-view",
        &[
            "CREATE SCHEMA tx_ddl_view_before",
            "CREATE TABLE tx_ddl_view_before.tx_ddl_view_rows (id INT PRIMARY KEY)",
            "INSERT INTO tx_ddl_view_before.tx_ddl_view_rows VALUES (1)",
            "CREATE VIEW tx_ddl_view_before.tx_ddl_view AS \
             SELECT id FROM tx_ddl_view_before.tx_ddl_view_rows",
        ],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("ALTER SCHEMA tx_ddl_view_before RENAME TO tx_ddl_view_after")
        .expect("rename schema transactionally");
    let private = fixture
        .execute("SELECT id FROM tx_ddl_view_after.tx_ddl_view ORDER BY id")
        .expect("read the renamed view before commit")
        .rows()
        .to_vec();

    // Act
    fixture.execute("COMMIT").expect("commit the schema rename");
    let rows = fixture
        .execute("SELECT id FROM tx_ddl_view_after.tx_ddl_view ORDER BY id")
        .expect("read the view after the schema rename")
        .rows()
        .to_vec();

    // Assert
    let expected = vec![vec![Value::Int64(1)]];
    assert_eq!(private, expected);
    assert_eq!(rows, expected);
}

#[test]
fn should_preserve_staged_values_after_transactional_column_rename() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-column-rename-after-staged-write",
        &["CREATE TABLE tx_ddl_column_rows (id INT PRIMARY KEY, old_label TEXT)"],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("INSERT INTO tx_ddl_column_rows VALUES (1, 'staged')")
        .expect("stage a row write");
    fixture
        .execute("ALTER TABLE tx_ddl_column_rows RENAME COLUMN old_label TO new_label")
        .expect("rename column transactionally");

    // Act
    fixture
        .execute("COMMIT")
        .expect("commit the staged row and column rename");
    let rows = fixture.rows("SELECT id, new_label FROM tx_ddl_column_rows ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Int64(1), Value::String("staged".to_owned())]]
    );
}

#[test]
fn should_resolve_staged_conflict_after_transactional_target_column_rename() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-conflict-target-column-rename",
        &[
            "CREATE TABLE tx_ddl_conflict_rows \
               (id INT PRIMARY KEY, old_label TEXT UNIQUE)",
            "INSERT INTO tx_ddl_conflict_rows VALUES (1, 'original')",
        ],
    );
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute(
            "INSERT INTO tx_ddl_conflict_rows VALUES (2, 'original') \
             ON CONFLICT (old_label) DO UPDATE SET \
               id = excluded.id, old_label = excluded.old_label",
        )
        .expect("stage a conflicting row write");
    fixture
        .execute("ALTER TABLE tx_ddl_conflict_rows RENAME COLUMN old_label TO new_label")
        .expect("rename the conflict target column transactionally");

    // Act
    fixture
        .execute("COMMIT")
        .expect("resolve the conflict and commit the renamed column");
    let rows = fixture.rows("SELECT id, new_label FROM tx_ddl_conflict_rows ORDER BY id");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Int64(2), Value::String("original".to_owned())]]
    );
}

#[test]
fn should_rollback_materialized_view_publication_with_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-materialized-view-rollback",
        &[
            "CREATE TABLE public.issue833_mv_rollback_source \
               (id INT PRIMARY KEY, label TEXT NOT NULL)",
            "INSERT INTO public.issue833_mv_rollback_source VALUES (1, 'retained')",
        ],
    );
    let observer = fixture.cassie.create_session("observer", None);
    fixture.execute("BEGIN").expect("begin the DDL transaction");

    // Act
    fixture
        .execute(
            "CREATE MATERIALIZED VIEW public.issue833_transactional_materialized_view AS \
             SELECT id, label FROM public.issue833_mv_rollback_source",
        )
        .expect("create and populate a materialized view inside the transaction");
    let writer_view = visible_materialized_view(&fixture.cassie, &fixture.session);
    let observer_view_before_commit = visible_materialized_view(&fixture.cassie, &observer);
    let materialized_rows = fixture
        .rows("SELECT id, label FROM public.issue833_transactional_materialized_view ORDER BY id");
    fixture
        .execute("ROLLBACK")
        .expect("roll back materialized-view data and catalog publication");
    let writer_view_after_rollback = visible_materialized_view(&fixture.cassie, &fixture.session);
    let observer_view_after_rollback = visible_materialized_view(&fixture.cassie, &observer);
    let source_rows =
        fixture.rows("SELECT id, label FROM public.issue833_mv_rollback_source ORDER BY id");

    // Assert
    let materialized_view = vec![vec![Value::String(MATERIALIZED_VIEW_NAME.to_owned())]];
    let no_materialized_view: &[Vec<Value>] = &[];
    assert_eq!(writer_view, materialized_view);
    assert_eq!(observer_view_before_commit, no_materialized_view);
    assert_eq!(
        materialized_rows,
        vec![vec![Value::Int64(1), Value::String("retained".to_owned())]]
    );
    assert_eq!(writer_view_after_rollback, no_materialized_view);
    assert_eq!(observer_view_after_rollback, no_materialized_view);
    assert_eq!(
        source_rows,
        vec![vec![Value::Int64(1), Value::String("retained".to_owned())]]
    );
}

#[test]
fn should_publish_materialized_view_after_commit() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-materialized-view-commit",
        &[
            "CREATE TABLE public.issue833_mv_commit_source \
               (id INT PRIMARY KEY, label TEXT NOT NULL)",
            "INSERT INTO public.issue833_mv_commit_source VALUES (1, 'committed')",
        ],
    );
    let observer = fixture.cassie.create_session("observer", None);
    fixture.execute("BEGIN").expect("begin the DDL transaction");

    // Act
    fixture
        .execute(
            "CREATE MATERIALIZED VIEW public.issue833_transactional_materialized_view AS \
             SELECT id, label FROM public.issue833_mv_commit_source",
        )
        .expect("create and populate materialized view in transaction");
    let writer_view = visible_materialized_view(&fixture.cassie, &fixture.session);
    let observer_view_before_commit = visible_materialized_view(&fixture.cassie, &observer);
    fixture.execute("COMMIT").expect("commit materialized view");
    let observer_view_after_commit = visible_materialized_view(&fixture.cassie, &observer);
    let observer_rows = fixture
        .cassie
        .execute_sql(
            &observer,
            "SELECT id, label \
             FROM public.issue833_transactional_materialized_view ORDER BY id",
            vec![],
        )
        .expect("read committed materialized-view output")
        .rows()
        .to_vec();

    // Assert
    let materialized_view = vec![vec![Value::String(MATERIALIZED_VIEW_NAME.to_owned())]];
    let no_materialized_view: &[Vec<Value>] = &[];
    assert_eq!(writer_view, materialized_view);
    assert_eq!(observer_view_before_commit, no_materialized_view);
    assert_eq!(observer_view_after_commit, materialized_view);
    assert_eq!(
        observer_rows,
        vec![vec![Value::Int64(1), Value::String("committed".to_owned())]]
    );
}

#[test]
fn should_remove_failed_materialized_view_creation() {
    // Arrange
    let fixture = sql_fixture("issue-826-materialized-view-failed-create", &[]);
    let name = "public.issue826_failed_materialized_view";

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "CREATE MATERIALIZED VIEW public.issue826_failed_materialized_view AS \
         SELECT 1 / 0 AS value",
        vec![],
    );
    let visible_views = fixture.rows(
        "SELECT matviewname FROM pg_catalog.pg_matviews \
         WHERE matviewname = 'issue826_failed_materialized_view'",
    );
    let stored_metadata = fixture.cassie.catalog.get_materialized_projection(name);

    // Assert
    assert!(result.is_err(), "the view query divides by zero");
    assert_eq!(visible_views, Vec::<Vec<Value>>::new());
    assert!(stored_metadata.is_none());
}

#[test]
fn should_rollback_materialized_view_refresh() {
    // Arrange
    let fixture = sql_fixture(
        "issue-833-materialized-view-refresh-rollback",
        &[
            "CREATE TABLE public.issue833_mv_refresh_source \
               (id INT PRIMARY KEY, label TEXT NOT NULL)",
            "INSERT INTO public.issue833_mv_refresh_source VALUES (1, 'retained')",
            "CREATE MATERIALIZED VIEW public.issue833_mv_refresh_view AS \
               SELECT id, label FROM public.issue833_mv_refresh_source WITH NO DATA",
        ],
    );
    let observer = fixture.cassie.create_session("observer", None);
    fixture.execute("BEGIN").expect("begin refresh transaction");

    // Act
    fixture
        .execute("REFRESH MATERIALIZED VIEW public.issue833_mv_refresh_view")
        .expect("refresh materialized view in transaction");
    let writer_population = fixture.rows(
        "SELECT ispopulated FROM pg_catalog.pg_matviews \
         WHERE matviewname = 'issue833_mv_refresh_view'",
    );
    let writer_rows =
        fixture.rows("SELECT id, label FROM public.issue833_mv_refresh_view ORDER BY id");
    let observer_population = fixture
        .cassie
        .execute_sql(
            &observer,
            "SELECT ispopulated FROM pg_catalog.pg_matviews \
             WHERE matviewname = 'issue833_mv_refresh_view'",
            vec![],
        )
        .expect("read observer materialized-view population")
        .rows()
        .to_vec();
    let observer_scan = fixture.cassie.execute_sql(
        &observer,
        "SELECT id, label FROM public.issue833_mv_refresh_view",
        vec![],
    );
    fixture.execute("ROLLBACK").expect("roll back refresh");
    let writer_population_after_rollback = fixture.rows(
        "SELECT ispopulated FROM pg_catalog.pg_matviews \
         WHERE matviewname = 'issue833_mv_refresh_view'",
    );
    let writer_scan_after_rollback = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id, label FROM public.issue833_mv_refresh_view",
        vec![],
    );

    // Assert
    assert_eq!(writer_population, vec![vec![Value::Bool(true)]]);
    assert_eq!(
        writer_rows,
        vec![vec![Value::Int64(1), Value::String("retained".to_owned())]]
    );
    assert_eq!(observer_population, vec![vec![Value::Bool(false)]]);
    assert!(
        observer_scan.is_err(),
        "observer should see the unpopulated view"
    );
    assert_eq!(
        writer_population_after_rollback,
        vec![vec![Value::Bool(false)]]
    );
    assert!(
        writer_scan_after_rollback.is_err(),
        "rollback should restore the unpopulated view"
    );
}

#[test]
fn should_reject_view_commands_for_materialized_projection() {
    // Arrange
    let fixture = sql_fixture(
        "issue-826-materialized-projection-view-commands",
        &[
            "CREATE TABLE public.issue826_projection_source (id INT PRIMARY KEY)",
            "INSERT INTO public.issue826_projection_source VALUES (1)",
            "CREATE MATERIALIZED PROJECTION public.issue826_projection \
               AS SELECT id FROM public.issue826_projection_source",
        ],
    );

    // Act
    let refresh_result = fixture.cassie.execute_sql(
        &fixture.session,
        "REFRESH MATERIALIZED VIEW public.issue826_projection",
        vec![],
    );
    let drop_result = fixture.cassie.execute_sql(
        &fixture.session,
        "DROP MATERIALIZED VIEW public.issue826_projection",
        vec![],
    );
    let drop_if_exists_result = fixture.cassie.execute_sql(
        &fixture.session,
        "DROP MATERIALIZED VIEW IF EXISTS public.issue826_projection",
        vec![],
    );
    let projection = fixture
        .cassie
        .catalog
        .get_materialized_projection("public.issue826_projection");

    // Assert
    assert!(refresh_result.is_err());
    assert!(drop_result.is_err());
    assert!(drop_if_exists_result.is_ok());
    assert!(projection.is_some());
}

#[test]
fn should_report_unpopulated_view_scan_inside_transaction() {
    // Arrange
    let fixture = sql_fixture(
        "issue-826-unpopulated-view-transaction-scan",
        &["CREATE TABLE public.issue826_unpopulated_source (id INT PRIMARY KEY)"],
    );
    let observer = fixture.cassie.create_session("observer", None);
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute(
            "CREATE MATERIALIZED VIEW public.issue826_unpopulated_view AS \
             SELECT id FROM public.issue826_unpopulated_source WITH NO DATA",
        )
        .expect("create unpopulated materialized view");

    // Act
    let writer_scan = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id FROM public.issue826_unpopulated_view",
        vec![],
    );
    let observer_catalog = fixture
        .cassie
        .execute_sql(
            &observer,
            "SELECT matviewname FROM pg_catalog.pg_matviews \
             WHERE matviewname = 'issue826_unpopulated_view'",
            vec![],
        )
        .expect("read observer catalog");
    fixture
        .execute("ROLLBACK")
        .expect("roll back view creation");
    let writer_catalog_after_rollback = fixture.rows(
        "SELECT matviewname FROM pg_catalog.pg_matviews \
         WHERE matviewname = 'issue826_unpopulated_view'",
    );

    // Assert
    assert!(matches!(
        writer_scan,
        Err(CassieError::Diagnostic(error)) if error.sql_state == "55000"
    ));
    let no_catalog_rows: &[Vec<Value>] = &[];
    assert_eq!(observer_catalog.rows(), no_catalog_rows);
    assert_eq!(writer_catalog_after_rollback, Vec::<Vec<Value>>::new());
}
