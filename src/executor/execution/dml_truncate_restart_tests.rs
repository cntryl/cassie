//! A transactional `TRUNCATE ... RESTART IDENTITY` is staged with its rows.

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

fn fixture(prefix: &str) -> (Cassie, CassieSession, String) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("owner", None);
    let sequence = format!("{prefix}_seq");
    for sql in [
        format!("CREATE SEQUENCE {sequence}"),
        format!("CREATE TABLE {prefix}_rows (id BIGINT DEFAULT nextval('{sequence}'))"),
        format!("ALTER SEQUENCE {sequence} OWNED BY {prefix}_rows.id"),
        format!("INSERT INTO {prefix}_rows DEFAULT VALUES"),
        format!("INSERT INTO {prefix}_rows DEFAULT VALUES"),
    ] {
        cassie.execute_sql(&session, &sql, vec![]).unwrap();
    }
    (cassie, session, sequence)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

fn next(cassie: &Cassie, session: &CassieSession, sequence: &str) -> Value {
    cassie
        .execute_sql(session, &format!("SELECT nextval('{sequence}')"), vec![])
        .unwrap()
        .rows()[0][0]
        .clone()
}

fn inserted_id(cassie: &Cassie, session: &CassieSession, prefix: &str) -> Value {
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {prefix}_rows DEFAULT VALUES RETURNING id"),
            vec![],
        )
        .unwrap()
        .rows()[0][0]
        .clone()
}

#[test]
fn should_keep_staged_restart_invisible_to_other_sessions_until_commit() {
    // Arrange
    let (cassie, session, sequence) = fixture("restart_private");
    let other = cassie.create_session("other", None);
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "TRUNCATE restart_private_rows RESTART IDENTITY",
    );

    // Act
    let concurrent = next(&cassie, &other, &sequence);
    run(&cassie, &session, "ROLLBACK");
    let after_rollback = next(&cassie, &other, &sequence);

    // Assert
    assert_eq!(concurrent, Value::Int64(3));
    assert_eq!(after_rollback, Value::Int64(4));
}

#[test]
fn should_publish_staged_restart_with_the_truncation_at_commit() {
    // Arrange
    let (cassie, session, _sequence) = fixture("restart_commit");
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "TRUNCATE restart_commit_rows RESTART IDENTITY",
    );

    // Act
    let inside = inserted_id(&cassie, &session, "restart_commit");
    run(&cassie, &session, "COMMIT");
    let after = inserted_id(&cassie, &session, "restart_commit");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id FROM restart_commit_rows ORDER BY id",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();

    // Assert
    assert_eq!(inside, Value::Int64(1));
    assert_eq!(after, Value::Int64(2));
    assert_eq!(rows, vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]);
}

#[test]
fn should_discard_staged_restart_when_the_transaction_rolls_back() {
    // Arrange
    let (cassie, session, _sequence) = fixture("restart_rollback");
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "TRUNCATE restart_rollback_rows RESTART IDENTITY",
    );

    // Act
    run(&cassie, &session, "ROLLBACK");
    let after = inserted_id(&cassie, &session, "restart_rollback");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM restart_rollback_rows",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec();

    // Assert
    assert_eq!(after, Value::Int64(3));
    assert_eq!(rows, vec![vec![Value::Int64(3)]]);
}

#[test]
fn should_restore_staged_restart_on_savepoint_rollback() {
    // Arrange
    let (cassie, session, _sequence) = fixture("restart_savepoint");
    run(&cassie, &session, "BEGIN");
    run(&cassie, &session, "SAVEPOINT before_truncate");
    run(
        &cassie,
        &session,
        "TRUNCATE restart_savepoint_rows RESTART IDENTITY",
    );

    // Act
    run(&cassie, &session, "ROLLBACK TO SAVEPOINT before_truncate");
    let inside = inserted_id(&cassie, &session, "restart_savepoint");
    run(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(inside, Value::Int64(3));
}

#[test]
fn should_leave_a_later_transaction_untouched_by_an_earlier_rolled_back_restart() {
    // Arrange
    let (cassie, session, sequence) = fixture("restart_later");
    let clone = session.clone();
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "TRUNCATE restart_later_rows RESTART IDENTITY",
    );
    run(&cassie, &session, "ROLLBACK");

    // Act
    run(&cassie, &clone, "BEGIN");
    let allocated = inserted_id(&cassie, &clone, "restart_later");
    run(&cassie, &clone, "ROLLBACK");
    let after = next(&cassie, &session, &sequence);

    // Assert
    assert_eq!(allocated, Value::Int64(3));
    assert_eq!(after, Value::Int64(4));
}

#[test]
fn should_restart_immediately_outside_a_transaction() {
    // Arrange
    let (cassie, session, _sequence) = fixture("restart_autocommit");

    // Act
    run(
        &cassie,
        &session,
        "TRUNCATE restart_autocommit_rows RESTART IDENTITY",
    );
    let after = inserted_id(&cassie, &session, "restart_autocommit");

    // Assert
    assert_eq!(after, Value::Int64(1));
}

#[test]
fn should_reject_commit_when_another_session_advanced_the_restarted_sequence() {
    // Arrange
    let (cassie, session, sequence) = fixture("restart_race");
    let other = cassie.create_session("other", None);
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "TRUNCATE restart_race_rows RESTART IDENTITY",
    );
    let concurrent = next(&cassie, &other, &sequence);

    // Act
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
    let rows = cassie
        .execute_sql(&other, "SELECT count(*) FROM restart_race_rows", vec![])
        .unwrap()
        .rows()
        .to_vec();
    let after = next(&cassie, &other, &sequence);

    // Assert
    assert_eq!(concurrent, Value::Int64(3));
    assert!(commit.is_err());
    assert!(!session.is_transaction_active());
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
    assert_eq!(after, Value::Int64(4));
}
