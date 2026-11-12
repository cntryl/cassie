//! Composite types and their values survive restart on local storage.

use crate::support_composite_types::{first_column_text, only_text, LocalStore};
use cassie::app::Cassie;

fn run(cassie: &Cassie, sql: &str) -> Vec<Vec<cassie::types::Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, vec![])
        .unwrap_or_else(|_| panic!("statement failed: {sql}"))
        .rows()
        .to_vec()
}

#[test]
fn should_keep_types_rows_plus_catalog_identity_across_restart() {
    // Arrange
    let store = LocalStore::new("composite_restart");
    let before_oid;
    {
        let cassie = store.open();
        run(&cassie, "CREATE TYPE address AS (street text, zip int)");
        run(&cassie, "CREATE TABLE homes (id int, a address)");
        run(
            &cassie,
            "INSERT INTO homes VALUES (1, ROW('x y', 1)), (2, NULL), (3, ROW(NULL, NULL))",
        );
        before_oid = only_text(&run(
            &cassie,
            "SELECT oid::text FROM pg_type WHERE typname = 'address'",
        ));
    }

    // Act
    let cassie = store.open();
    let rows = first_column_text(&run(&cassie, "SELECT a::text FROM homes ORDER BY id"));
    let after_oid = only_text(&run(
        &cassie,
        "SELECT oid::text FROM pg_type WHERE typname = 'address'",
    ));
    let still_cast = only_text(&run(&cassie, "SELECT '(z,9)'::address::text"));

    // Assert
    assert_eq!(
        rows,
        vec![Some("(\"x y\",1)".to_owned()), None, Some("(,)".to_owned())]
    );
    assert!(before_oid.is_some(), "type OID is allocated");
    assert_eq!(before_oid, after_oid, "type OID is durable");
    assert_eq!(still_cast, Some("(z,9)".to_owned()));
}

#[test]
fn should_not_restore_a_dropped_type_after_restart() {
    // Arrange
    let store = LocalStore::new("composite_restart_drop");
    {
        let cassie = store.open();
        run(&cassie, "CREATE TYPE address AS (street text, zip int)");
        run(&cassie, "DROP TYPE address");
    }

    // Act
    let cassie = store.open();
    let session = cassie.create_session("tester", None);
    let resolved = cassie.execute_sql(&session, "SELECT '(a,1)'::address::text", vec![]);

    // Assert
    assert!(resolved.is_err(), "dropped type must stay dropped");
}

#[test]
fn should_not_persist_a_rolled_back_create_type() {
    // Arrange
    let store = LocalStore::new("composite_restart_rollback");
    {
        let cassie = store.open();
        let session = cassie.create_session("tester", None);
        for sql in [
            "BEGIN",
            "CREATE TYPE address AS (street text, zip int)",
            "ROLLBACK",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
    }

    // Act
    let cassie = store.open();
    let session = cassie.create_session("tester", None);
    let recreated = cassie.execute_sql(
        &session,
        "CREATE TYPE address AS (street text, zip int)",
        vec![],
    );

    // Assert
    assert!(recreated.is_ok(), "rolled-back type left no durable trace");
}
