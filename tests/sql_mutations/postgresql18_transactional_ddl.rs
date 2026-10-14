//! PostgreSQL 18.6 transactional catalog publication tracked by #833.

use super::support_sql_fixture::sql_fixture;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const MATERIALIZED_VIEW_NAME: &str = "issue833_transactional_materialized_view";

#[path = "postgresql18_transactional_ddl/alter_rollback_visibility.rs"]
mod alter_rollback_visibility;

#[path = "postgresql18_transactional_ddl/catalog_snapshot.rs"]
mod catalog_snapshot;

#[path = "postgresql18_transactional_ddl/restart_publication.rs"]
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

fn enum_visibility_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT value AS enum_value, \
                    (SELECT enumlabel FROM pg_catalog.pg_enum \
                     WHERE enumtypid = 'public.issue833_enum_visibility_wave36'::regtype \
                     ORDER BY enumsortorder LIMIT 1) AS catalog_label \
             FROM public.issue833_enum_visibility_wave36_rows",
            vec![],
        )
        .expect("query enum output and the SQL catalog snapshot")
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
fn should_keep_enum_output_fresh_while_catalog_scan_uses_transaction_snapshot() {
    // This #833 visibility scenario depends on #827 enum DDL. Cassie currently
    // rejects CREATE TYPE before either session reaches the snapshot checks,
    // so the enum/catalog visibility assertions remain masked by that gap.
    // Arrange
    let fixture = sql_fixture("issue-833-enum-snapshot-publication", &[]);
    let reader = fixture.session.clone();
    let writer = fixture.cassie.create_session("writer", None);
    for statement in [
        "CREATE TYPE public.issue833_enum_visibility_wave36 AS ENUM ('old', 'later')",
        "CREATE TABLE public.issue833_enum_visibility_wave36_rows \
           (value public.issue833_enum_visibility_wave36)",
        "INSERT INTO public.issue833_enum_visibility_wave36_rows VALUES ('old')",
    ] {
        fixture
            .cassie
            .execute_sql(&writer, statement, vec![])
            .unwrap_or_else(|error| {
                panic!("enum visibility setup failed for {statement}: {error}")
            });
    }

    // Act
    fixture
        .cassie
        .execute_sql(
            &reader,
            "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            vec![],
        )
        .expect("begin the repeatable-read snapshot");
    let repeatable_read_before = enum_visibility_rows(&fixture.cassie, &reader);
    fixture
        .cassie
        .execute_sql(
            &writer,
            "ALTER TYPE public.issue833_enum_visibility_wave36 RENAME VALUE 'old' TO 'new'",
            vec![],
        )
        .expect("rename enum value in another session");
    let repeatable_read_after = enum_visibility_rows(&fixture.cassie, &reader);
    fixture
        .cassie
        .execute_sql(&reader, "COMMIT", vec![])
        .expect("finish the repeatable-read snapshot");

    fixture
        .cassie
        .execute_sql(
            &reader,
            "BEGIN TRANSACTION ISOLATION LEVEL READ COMMITTED",
            vec![],
        )
        .expect("begin the read-committed transaction");
    let read_committed_before = enum_visibility_rows(&fixture.cassie, &reader);
    fixture
        .cassie
        .execute_sql(
            &writer,
            "ALTER TYPE public.issue833_enum_visibility_wave36 RENAME VALUE 'new' TO 'latest'",
            vec![],
        )
        .expect("rename another enum value in the writer session");
    let read_committed_after = enum_visibility_rows(&fixture.cassie, &reader);
    fixture
        .cassie
        .execute_sql(&reader, "COMMIT", vec![])
        .expect("finish the read-committed transaction");

    // Assert
    assert_eq!(
        [
            repeatable_read_before,
            repeatable_read_after,
            read_committed_before,
            read_committed_after,
        ],
        [
            vec![vec![
                Value::String("old".to_owned()),
                Value::String("old".to_owned())
            ]],
            vec![vec![
                Value::String("new".to_owned()),
                Value::String("old".to_owned())
            ]],
            vec![vec![
                Value::String("new".to_owned()),
                Value::String("new".to_owned())
            ]],
            vec![vec![
                Value::String("latest".to_owned()),
                Value::String("latest".to_owned())
            ]],
        ],
        "enum output and pg_enum SQL snapshots must follow PostgreSQL 18.6 visibility rules"
    );
}
