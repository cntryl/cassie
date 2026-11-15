//! Implicit `SERIAL` and identity sequences belong to their table's database.

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

fn fixture() -> (Cassie, CassieSession, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let default = cassie.create_session("root", None);
    cassie
        .execute_sql(&default, "CREATE DATABASE extra_db", vec![])
        .unwrap();
    let extra = cassie.create_session("root", Some("extra_db".to_owned()));
    (cassie, default, extra)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

fn sequence_names(cassie: &Cassie) -> Vec<String> {
    cassie
        .catalog
        .list_sequences()
        .into_iter()
        .map(|sequence| sequence.name)
        .collect()
}

fn inserted_id(cassie: &Cassie, session: &CassieSession, table: &str) -> Value {
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {table} DEFAULT VALUES RETURNING id"),
            vec![],
        )
        .unwrap()
        .rows()[0][0]
        .clone()
}

#[test]
fn should_create_serial_sequence_in_the_table_database() {
    // Arrange
    let (cassie, _default, extra) = fixture();

    // Act
    run(
        &cassie,
        &extra,
        "CREATE TABLE notes (id SERIAL PRIMARY KEY)",
    );
    let names = sequence_names(&cassie);
    let first = inserted_id(&cassie, &extra, "notes");

    // Assert
    assert_eq!(names, vec!["extra_db.public.notes_id_seq".to_owned()]);
    assert_eq!(first, Value::Int64(1));
}

#[test]
fn should_create_identity_sequence_in_the_table_database() {
    // Arrange
    let (cassie, _default, extra) = fixture();

    // Act
    run(
        &cassie,
        &extra,
        "CREATE TABLE items (id BIGINT GENERATED ALWAYS AS IDENTITY)",
    );
    let names = sequence_names(&cassie);
    let first = inserted_id(&cassie, &extra, "items");

    // Assert
    assert_eq!(names, vec!["extra_db.public.items_id_seq".to_owned()]);
    assert_eq!(first, Value::Int64(1));
}

#[test]
fn should_create_serial_table_inside_a_transaction_in_a_non_default_database() {
    // Arrange
    let (cassie, _default, extra) = fixture();
    run(&cassie, &extra, "BEGIN");

    // Act
    let created = cassie.execute_sql(
        &extra,
        "CREATE TABLE txn_notes (id SERIAL PRIMARY KEY)",
        vec![],
    );
    let first = inserted_id(&cassie, &extra, "txn_notes");
    run(&cassie, &extra, "COMMIT");
    let second = inserted_id(&cassie, &extra, "txn_notes");

    // Assert
    assert!(created.is_ok());
    assert_eq!(first, Value::Int64(1));
    assert_eq!(second, Value::Int64(2));
}

#[test]
fn should_keep_same_named_serial_tables_independent_across_databases() {
    // Arrange
    let (cassie, default, extra) = fixture();
    run(
        &cassie,
        &extra,
        "CREATE TABLE users (id SERIAL PRIMARY KEY)",
    );
    inserted_id(&cassie, &extra, "users");
    inserted_id(&cassie, &extra, "users");

    // Act
    let created = cassie.execute_sql(
        &default,
        "CREATE TABLE users (id SERIAL PRIMARY KEY)",
        vec![],
    );
    let default_first = inserted_id(&cassie, &default, "users");
    let extra_next = inserted_id(&cassie, &extra, "users");

    // Assert
    assert!(created.is_ok());
    assert_eq!(default_first, Value::Int64(1));
    assert_eq!(extra_next, Value::Int64(3));
}

#[test]
fn should_not_resolve_another_database_sequence_through_nextval() {
    // Arrange
    let (cassie, default, extra) = fixture();
    run(
        &cassie,
        &extra,
        "CREATE TABLE notes (id SERIAL PRIMARY KEY)",
    );

    // Act
    let leaked = cassie.execute_sql(&default, "SELECT nextval('notes_id_seq')", vec![]);
    let first = inserted_id(&cassie, &extra, "notes");

    // Assert
    assert!(leaked.is_err());
    assert_eq!(first, Value::Int64(1));
}

#[test]
fn should_keep_the_bare_sequence_name_in_the_default_database() {
    // Arrange
    let (cassie, default, _extra) = fixture();

    // Act
    run(
        &cassie,
        &default,
        "CREATE TABLE plain (id SERIAL PRIMARY KEY)",
    );
    let names = sequence_names(&cassie);
    let default_expression = cassie
        .catalog
        .get_constraints(&cassie.midge.canonical_collection_name("plain"))
        .into_iter()
        .find_map(|constraint| constraint.default_sequence);

    // Assert
    assert_eq!(names, vec!["postgres.public.plain_id_seq".to_owned()]);
    assert_eq!(default_expression.as_deref(), Some("plain_id_seq"));
}

#[test]
fn should_add_a_serial_column_sequence_in_the_table_database() {
    // Arrange
    let (cassie, _default, extra) = fixture();
    run(&cassie, &extra, "CREATE TABLE widgets (name TEXT)");

    // Act
    run(&cassie, &extra, "ALTER TABLE widgets ADD COLUMN id SERIAL");
    let names = sequence_names(&cassie);

    // Assert
    assert_eq!(names, vec!["extra_db.public.widgets_id_seq".to_owned()]);
}

#[test]
fn should_keep_altered_table_sequences_in_the_table_database() {
    // Arrange
    let (cassie, _default, extra) = fixture();
    for sql in [
        "CREATE TABLE plain (id BIGINT NOT NULL, name TEXT)",
        "CREATE TABLE renamed_from (id SERIAL PRIMARY KEY)",
        "CREATE SCHEMA app",
    ] {
        run(&cassie, &extra, sql);
    }

    // Act
    for sql in [
        "ALTER TABLE plain ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY",
        "ALTER TABLE renamed_from RENAME TO renamed_to",
        "CREATE TABLE app.scoped (id SERIAL PRIMARY KEY)",
    ] {
        run(&cassie, &extra, sql);
    }
    let names = sequence_names(&cassie);
    let renamed_first = inserted_id(&cassie, &extra, "renamed_to");

    // Assert
    assert_eq!(
        names,
        vec![
            "extra_db.app.scoped_id_seq".to_owned(),
            "extra_db.public.plain_id_seq".to_owned(),
            "extra_db.public.renamed_to_id_seq".to_owned(),
        ]
    );
    assert_eq!(renamed_first, Value::Int64(1));
}

#[test]
fn should_reject_bare_name_sequence_commands_from_another_database() {
    // Arrange
    let (cassie, default, extra) = fixture();
    run(
        &cassie,
        &extra,
        "CREATE TABLE source (id SERIAL PRIMARY KEY)",
    );

    // Act
    let rejected = [
        "DROP SEQUENCE source_id_seq",
        "ALTER SEQUENCE source_id_seq RESTART WITH 50",
        "SELECT currval('source_id_seq')",
        "SELECT setval('source_id_seq', 99)",
        "CREATE TABLE borrow (id BIGINT DEFAULT nextval('source_id_seq'))",
    ]
    .map(|sql| cassie.execute_sql(&default, sql, vec![]).is_err());
    let first = inserted_id(&cassie, &extra, "source");

    // Assert
    assert_eq!(rejected, [true; 5]);
    assert_eq!(first, Value::Int64(1));
}
