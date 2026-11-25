//! A role without sequence privileges cannot read or advance sequence state.

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::Value;

pub(crate) fn fixture(extra_setup: &[&str]) -> (Cassie, CassieSession, CassieSession) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let root = cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("root");
    for setup in [
        "CREATE SEQUENCE ps",
        "SELECT nextval('ps')",
        "CREATE TABLE pt (v INT)",
        "INSERT INTO pt VALUES (1)",
        "CREATE TABLE secret (ssn TEXT)",
        "INSERT INTO secret VALUES ('123-45-6789')",
    ]
    .iter()
    .chain(extra_setup)
    {
        cassie.execute_sql(&root, setup, vec![]).expect(setup);
    }
    cassie
        .create_role("alice", true, Some("fixture-password".into()), false)
        .expect("create alice");
    for grant in [
        "GRANT USAGE ON SCHEMA public TO alice",
        "GRANT SELECT ON TABLE public.pt TO alice",
    ] {
        cassie.execute_sql(&root, grant, vec![]).expect(grant);
    }
    let alice = cassie
        .authenticate_role("alice", Some("fixture-password"), None)
        .expect("alice");
    (cassie, root, alice)
}

pub(crate) fn denied(cassie: &Cassie, session: &CassieSession, sql: &str) -> bool {
    matches!(
        cassie.execute_sql(session, sql, vec![]),
        Err(CassieError::InsufficientPrivilege)
    )
}

pub(crate) fn scalar(cassie: &Cassie, session: &CassieSession, sql: &str) -> Value {
    let result = cassie.execute_sql(session, sql, vec![]).expect(sql);
    result.rows()[0][0].clone()
}

pub(crate) const LAST_VALUE: &str =
    "SELECT last_value FROM pg_catalog.pg_sequences WHERE sequencename = 'ps'";

#[test]
fn should_hide_sequence_last_value_from_a_role_without_access() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);

    // Act
    let value = scalar(&cassie, &alice, LAST_VALUE);

    // Assert
    assert_eq!(value, Value::Null);
}

#[test]
fn should_show_sequence_last_value_to_an_administrator() {
    // Arrange
    let (cassie, root, _alice) = fixture(&[]);

    // Act
    let value = scalar(&cassie, &root, LAST_VALUE);

    // Assert
    assert_eq!(value, Value::Int64(1));
}

#[test]
fn should_deny_direct_nextval_for_a_role_without_access() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);

    // Act
    let refused = denied(&cassie, &alice, "SELECT nextval('ps')");

    // Assert
    assert!(refused);
}

#[test]
fn should_not_expose_sequence_state_through_pg_sequence_last_value() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);

    // Act
    let result = cassie.execute_sql(
        &alice,
        "SELECT pg_sequence_last_value('ps'::regclass)",
        vec![],
    );

    // Assert
    // The function is not implemented, so it must not return a value.
    assert!(!result.is_ok_and(|result| result.rows()[0][0] != Value::Null));
}

#[test]
fn should_deny_currval_for_a_role_without_access() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);

    // Act
    let refused = denied(&cassie, &alice, "SELECT currval('ps')");

    // Assert
    assert!(refused);
}

#[test]
fn should_deny_a_function_body_that_calls_nextval() {
    // Arrange
    let (cassie, root, alice) =
        fixture(&["CREATE FUNCTION bump() RETURNS BIGINT AS \"nextval('ps')\""]);

    // Act
    let refused = denied(&cassie, &alice, "SELECT bump()");
    let value = scalar(&cassie, &root, LAST_VALUE);

    // Assert
    assert!(refused);
    assert_eq!(value, Value::Int64(1));
}
