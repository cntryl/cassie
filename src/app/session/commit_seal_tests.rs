//! COMMIT seals its transaction instead of draining statement forks.

use super::CassieSession;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

fn fixture() -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("owner", None);
    cassie
        .execute_sql(&session, "CREATE TABLE seal_rows (value INT)", vec![])
        .unwrap();
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

fn values(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT value FROM seal_rows ORDER BY value",
            vec![],
        )
        .unwrap()
        .rows()
        .to_vec()
}

#[test]
fn should_reject_a_statement_fork_that_publishes_after_the_seal() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let early = cassie.new_statement_batch(Some(&session)).unwrap();
    let late = cassie.new_statement_batch(Some(&session)).unwrap();

    // Act
    let before_seal = session.publish_statement_batch(&early);
    let seal = session.seal_transaction_for_commit();
    let after_seal = session.publish_statement_batch(&late);
    drop(seal);

    // Assert
    assert!(before_seal.is_ok());
    assert!(after_seal.is_err());
}

#[test]
fn should_reject_a_projection_mode_declaration_after_the_seal() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let seal = session.seal_transaction_for_commit();

    // Act
    let declared = session.set_projection_write_mode(Some("replayable"));
    drop(seal);

    // Assert
    assert_eq!(declared.unwrap_err().sql_state(), "25000");
}

#[test]
fn should_release_the_seal_when_commit_leaves_the_transaction_open() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let batch = cassie.new_statement_batch(Some(&session)).unwrap();

    // Act
    drop(session.seal_transaction_for_commit());
    let published = session.publish_statement_batch(&batch);
    run(
        &cassie,
        &session,
        "INSERT INTO seal_rows (value) VALUES (1)",
    );
    run(&cassie, &session, "COMMIT");

    // Assert
    assert!(published.is_ok());
    assert_eq!(values(&cassie, &session), vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_not_seal_twice_or_unseal_an_outer_seal() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let outer = session.seal_transaction_for_commit();
    let batch = cassie.new_statement_batch(Some(&session)).unwrap();

    // Act
    drop(session.seal_transaction_for_commit());
    let while_outer_held = session.publish_statement_batch(&batch);
    drop(outer);

    // Assert
    assert!(while_outer_held.is_err());
}

#[test]
fn should_start_the_next_transaction_unsealed_after_commit() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "INSERT INTO seal_rows (value) VALUES (1)",
    );
    run(&cassie, &session, "COMMIT");

    // Act
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "INSERT INTO seal_rows (value) VALUES (2)",
    );
    run(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(
        values(&cassie, &session),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_keep_the_transaction_usable_after_a_failed_commit_is_rolled_back() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        "CREATE TABLE seal_unique (value INT UNIQUE)",
    );
    run(
        &cassie,
        &session,
        "INSERT INTO seal_unique (value) VALUES (1)",
    );
    let other = cassie.create_session("other", None);
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "INSERT INTO seal_unique (value) VALUES (2)",
    );
    run(
        &cassie,
        &other,
        "INSERT INTO seal_unique (value) VALUES (2)",
    );

    // Act
    let commit = cassie.execute_sql(&session, "COMMIT", vec![]);
    let _ = cassie.execute_sql(&session, "ROLLBACK", vec![]);
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "INSERT INTO seal_unique (value) VALUES (3)",
    );
    let second = cassie.execute_sql(&session, "COMMIT", vec![]);

    // Assert
    assert!(commit.is_err());
    assert!(second.is_ok());
}
