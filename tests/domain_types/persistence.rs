//! Domains, their rules and their typed rows survive restart on local storage.

use crate::support_composite_types::{only_text, LocalStore};
use cassie::app::Cassie;

fn run(cassie: &Cassie, sql: &str) -> Vec<Vec<cassie::types::Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, vec![])
        .unwrap_or_else(|_| panic!("statement failed: {sql}"))
        .rows()
        .to_vec()
}

fn seed(store: &LocalStore) -> Option<String> {
    let cassie = store.open();
    run(
        &cassie,
        "CREATE DOMAIN posint AS int DEFAULT 3 CHECK (VALUE > 0)",
    );
    run(&cassie, "CREATE TABLE items (id int, qty posint)");
    run(&cassie, "INSERT INTO items (id) VALUES (1)");
    only_text(&run(
        &cassie,
        "SELECT oid::text FROM pg_type WHERE typname = 'posint'",
    ))
}

#[test]
fn should_keep_the_domain_oid_across_restart() {
    // Arrange
    let store = LocalStore::new("domain_restart_oid");
    let before = seed(&store);

    // Act
    let cassie = store.open();
    let after = only_text(&run(
        &cassie,
        "SELECT oid::text FROM pg_type WHERE typname = 'posint'",
    ));

    // Assert
    assert!(before.is_some(), "domain OID is allocated");
    assert_eq!(before, after, "domain OID is durable");
}

#[test]
fn should_keep_enforcing_the_check_after_restart() {
    // Arrange
    let store = LocalStore::new("domain_restart_check");
    seed(&store);

    // Act
    let cassie = store.open();
    let session = cassie.create_session("tester", None);
    let rejected = cassie.execute_sql(&session, "INSERT INTO items VALUES (2, -1)", vec![]);

    // Assert
    assert!(rejected.is_err(), "the persisted CHECK still rejects");
}

#[test]
fn should_keep_applying_the_domain_default_after_restart() {
    // Arrange
    let store = LocalStore::new("domain_restart_default");
    seed(&store);

    // Act
    let cassie = store.open();
    run(&cassie, "INSERT INTO items (id) VALUES (2)");
    let value = only_text(&run(&cassie, "SELECT qty::text FROM items WHERE id = 2"));

    // Assert
    assert_eq!(value, Some("3".to_owned()));
}

#[test]
fn should_keep_rows_of_a_domain_column_across_restart() {
    // Arrange
    let store = LocalStore::new("domain_restart_rows");
    seed(&store);

    // Act
    let cassie = store.open();
    let value = only_text(&run(&cassie, "SELECT qty::text FROM items WHERE id = 1"));

    // Assert
    assert_eq!(value, Some("3".to_owned()));
}

#[test]
fn should_not_restore_a_dropped_domain_after_restart() {
    // Arrange
    let store = LocalStore::new("domain_restart_drop");
    {
        let cassie = store.open();
        run(&cassie, "CREATE DOMAIN posint AS int");
        run(&cassie, "DROP DOMAIN posint");
    }

    // Act
    let cassie = store.open();
    let session = cassie.create_session("tester", None);
    let resolved = cassie.execute_sql(&session, "SELECT (1)::posint", vec![]);

    // Assert
    assert!(resolved.is_err(), "dropped domain must stay dropped");
}

#[test]
fn should_not_persist_a_rolled_back_create_domain() {
    // Arrange
    let store = LocalStore::new("domain_restart_rollback");
    {
        let cassie = store.open();
        let session = cassie.create_session("tester", None);
        for sql in ["BEGIN", "CREATE DOMAIN posint AS int", "ROLLBACK"] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
    }

    // Act
    let cassie = store.open();
    let session = cassie.create_session("tester", None);
    let recreated = cassie.execute_sql(&session, "CREATE DOMAIN posint AS int", vec![]);

    // Assert
    assert!(
        recreated.is_ok(),
        "rolled-back domain left no durable trace"
    );
}

// Authored with the gate fixes: dependent row-type columns after hydration (review J1).

fn seed_row_type_column(store: &LocalStore) {
    let cassie = store.open();
    run(&cassie, "CREATE DOMAIN posint AS int CHECK (VALUE > 0)");
    run(&cassie, "CREATE TABLE dt (a posint)");
    run(&cassie, "CREATE TABLE holder (r dt)");
}

#[test]
fn should_stop_enforcing_a_dropped_check_on_a_row_type_column_after_restart() {
    // Arrange
    let store = LocalStore::new("domain_restart_row_column_drop");
    seed_row_type_column(&store);
    let cassie = store.open();
    run(&cassie, "ALTER DOMAIN posint DROP CONSTRAINT posint_check");
    let session = cassie.create_session("tester", None);

    // Act
    let inserted = cassie.execute_sql(&session, "INSERT INTO holder VALUES ('(-1)')", vec![]);

    // Assert
    assert!(
        inserted.is_ok(),
        "the dropped check must not be enforced from a re-embedded snapshot"
    );
}

#[test]
fn should_reconcile_no_columns_when_hydration_runs_again() {
    // Arrange
    let store = LocalStore::new("domain_restart_row_column_reconcile");
    seed_row_type_column(&store);
    let cassie = store.open();

    // Act
    let reconciled = cassie.catalog.reconcile_embedded_composites();

    // Assert
    assert_eq!(
        reconciled, 0,
        "flattened embedded row types are authoritative"
    );
}
