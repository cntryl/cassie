use super::support_sql as support;
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;
use support::*;

fn start(_label: &str) -> (Cassie, CassieSession) {
    let cassie = new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

#[test]
fn should_keep_renamed_table_in_its_source_schema() {
    // Arrange
    let (cassie, session) = start("rename_keeps_schema");
    run(&cassie, &session, "CREATE SCHEMA reporting");
    run(&cassie, &session, "CREATE TABLE reporting.orders (id INT)");
    run(
        &cassie,
        &session,
        "INSERT INTO reporting.orders (id) VALUES (7)",
    );

    // Act
    run(
        &cassie,
        &session,
        "ALTER TABLE reporting.orders RENAME TO archive",
    );

    // Assert
    let rows = cassie
        .execute_sql(&session, "SELECT id FROM reporting.archive", vec![])
        .expect("renamed table stays in reporting")
        .rows()
        .to_vec();
    assert_eq!(rows, vec![vec![Value::Int64(7)]]);
    assert!(cassie
        .execute_sql(&session, "SELECT id FROM public.archive", vec![])
        .is_err());
}

#[test]
fn should_reject_schema_qualified_rename_target() {
    // Arrange
    let (cassie, session) = start("rename_rejects_qualified_target");
    run(&cassie, &session, "CREATE SCHEMA reporting");
    run(&cassie, &session, "CREATE TABLE orders (id INT)");

    // Act
    let result = cassie.execute_sql(
        &session,
        "ALTER TABLE orders RENAME TO reporting.moved",
        vec![],
    );

    // Assert
    assert!(result.is_err());
    assert!(cassie
        .execute_sql(&session, "SELECT id FROM public.orders", vec![])
        .is_ok());
}

#[test]
fn should_drop_view_found_later_in_search_path() {
    // Arrange
    let (cassie, session) = start("drop_view_search_path");
    run(&cassie, &session, "CREATE SCHEMA beta");
    run(&cassie, &session, "CREATE TABLE beta.base (id INT)");
    run(
        &cassie,
        &session,
        "CREATE VIEW beta.v AS SELECT id FROM beta.base",
    );
    run(&cassie, &session, "SET search_path TO public, beta");

    // Act
    run(&cassie, &session, "DROP VIEW IF EXISTS v");

    // Assert
    assert!(cassie
        .execute_sql(&session, "SELECT id FROM v", vec![])
        .is_err());
}

#[test]
fn should_drop_projection_found_later_in_search_path() {
    // Arrange
    let (cassie, session) = start("drop_projection_search_path");
    run(&cassie, &session, "CREATE SCHEMA beta");
    run(&cassie, &session, "CREATE TABLE beta.base (id INT)");
    run(
        &cassie,
        &session,
        "CREATE MATERIALIZED PROJECTION beta.p AS SELECT id FROM beta.base",
    );
    run(&cassie, &session, "SET search_path TO public, beta");

    // Act
    let dropped = cassie.execute_sql(&session, "DROP MATERIALIZED PROJECTION p", vec![]);

    // Assert
    assert!(dropped.is_ok(), "projection p resolves through search_path");
}

#[test]
fn should_resolve_unqualified_catalog_names_to_pg_catalog_first() {
    // Arrange
    let (cassie, session) = start("pg_catalog_precedence");
    run(&cassie, &session, "CREATE TABLE pg_class (relname TEXT)");
    run(
        &cassie,
        &session,
        "INSERT INTO public.pg_class (relname) VALUES ('IMPOSTER')",
    );

    // Act
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT relname FROM pg_class WHERE relname = 'IMPOSTER'",
            vec![],
        )
        .expect("query pg_class")
        .rows()
        .to_vec();

    // Assert
    assert!(rows.is_empty(), "unqualified pg_class must be the catalog");
}

#[test]
fn should_accept_postgres_default_search_path() {
    // Arrange
    let (cassie, session) = start("default_search_path");

    // Act
    let set = cassie.execute_sql(&session, "SET search_path TO \"$user\", public", vec![]);

    // Assert
    assert!(set.is_ok(), "PostgreSQL's default search_path is accepted");
    let current = cassie
        .execute_sql(&session, "SELECT current_schema()", vec![])
        .expect("current_schema")
        .rows()
        .to_vec();
    assert_eq!(current, vec![vec![Value::String("public".into())]]);
    run(&cassie, &session, "CREATE TABLE lands_in_public (id INT)");
    assert!(cassie
        .execute_sql(&session, "SELECT id FROM public.lands_in_public", vec![])
        .is_ok());
}

#[test]
fn should_resolve_user_search_path_entry_to_role_schema() {
    // Arrange
    let (cassie, session) = start("user_search_path_schema");
    run(&cassie, &session, "CREATE SCHEMA tester");
    run(&cassie, &session, "SET search_path TO \"$user\", public");

    // Act
    run(&cassie, &session, "CREATE TABLE owned_by_user (id INT)");

    // Assert
    assert!(cassie
        .execute_sql(&session, "SELECT id FROM tester.owned_by_user", vec![])
        .is_ok());
}

#[test]
fn should_skip_missing_search_path_schemas_when_creating() {
    // Arrange
    let (cassie, session) = start("missing_search_path_schema_create");
    run(
        &cassie,
        &session,
        "SET search_path TO missing_schema, public",
    );

    // Act
    run(&cassie, &session, "CREATE TABLE skips_missing (id INT)");

    // Assert
    assert!(cassie
        .execute_sql(&session, "SELECT id FROM public.skips_missing", vec![])
        .is_ok());
}
#[test]
fn should_report_the_existing_schema_used_by_name_binding() {
    // Arrange
    let (cassie, session) = start("current_existing_schema");
    run(&cassie, &session, "CREATE SCHEMA tester");
    let raw = cassie::app::CassieSession::new("tester".into(), None);

    // Act
    for active in [&session, &raw] {
        for (search_path, expected) in [
            ("missing_schema, public", Value::String("public".into())),
            ("\"$user\", public", Value::String("tester".into())),
            ("missing_schema", Value::Null),
        ] {
            run(
                &cassie,
                active,
                &format!("SET search_path TO {search_path}"),
            );
            let result = cassie
                .execute_sql(active, "SELECT current_schema()", vec![])
                .expect("current schema");

            // Assert
            assert_eq!(result.rows(), vec![vec![expected]]);
        }
    }
    run(
        &cassie,
        &session,
        "SET search_path TO future_schema, public",
    );
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT current_schema()", vec![])
            .expect("before create")
            .rows(),
        vec![vec![Value::String("public".into())]]
    );
    run(&cassie, &session, "CREATE SCHEMA future_schema");
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT current_schema()", vec![])
            .expect("after create")
            .rows(),
        vec![vec![Value::String("future_schema".into())]]
    );
    run(&cassie, &session, "SET search_path TO tester, public");
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT current_schema()", vec![])
            .expect("before drop")
            .rows(),
        vec![vec![Value::String("tester".into())]]
    );
    run(&cassie, &session, "DROP SCHEMA tester");
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT current_schema()", vec![])
            .expect("after drop")
            .rows(),
        vec![vec![Value::String("public".into())]]
    );
    assert_eq!(
        cassie
            .execute_sql(&session, "SELECT pg_catalog.current_schema()", vec![])
            .expect("qualified current schema")
            .rows(),
        vec![vec![Value::String("public".into())]]
    );
}
