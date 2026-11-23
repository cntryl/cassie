//! COMMIT seals its transaction instead of draining statement forks.

use super::{CassieSession, SessionTransactionStatus};
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
    // Plain INSERT waits for the staged owner. Use COPY to create the
    // committed conflict independently, preserving the failed-COMMIT case.
    let copy = crate::sql::ast::CopyStatement {
        table: "seal_unique".to_owned(),
        columns: vec!["value".to_owned()],
        format: crate::sql::ast::CopyFormat::Csv,
        header: false,
    };
    cassie.copy_from_csv_stdin(&other, &copy, b"2\n").unwrap();

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

#[test]
fn should_refuse_transaction_control_while_a_commit_is_in_progress() {
    // Arrange
    let (cassie, session) = fixture();
    let cassie = std::sync::Arc::new(cassie);
    let clone = session.clone();
    run(&cassie, &session, "BEGIN");
    run(&cassie, &session, "SAVEPOINT held");
    let seal = session.seal_transaction_for_commit();
    let other_cassie = std::sync::Arc::clone(&cassie);

    // Act: a second client thread competes while this thread owns the COMMIT.
    let refused = std::thread::spawn(move || {
        [
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "SAVEPOINT s",
            "ROLLBACK TO SAVEPOINT held",
        ]
        .map(|sql| {
            other_cassie
                .execute_sql(&clone, sql, vec![])
                .map_err(|error| error.sql_state())
                .err()
        })
    })
    .join()
    .unwrap();
    let failed_while_sealed = session.is_transaction_failed();
    drop(seal);
    let after = cassie.execute_sql(&session, "ROLLBACK", vec![]);

    // Assert
    assert_eq!(refused, [Some("25000"); 5]);
    assert!(!failed_while_sealed);
    assert!(after.is_ok());
}

#[test]
fn should_not_retire_a_later_transaction_when_the_committed_one_is_gone() {
    // Arrange
    let (cassie, session) = fixture();
    let clone = session.clone();
    run(&cassie, &session, "BEGIN");
    let mut seal = session.seal_transaction_for_commit();
    clone.rollback_transaction();
    clone.begin_transaction(None).unwrap();
    run(&cassie, &clone, "INSERT INTO seal_rows (value) VALUES (7)");

    // Act
    seal.retire_committed();
    drop(seal);
    let still_active = clone.is_transaction_active();
    run(&cassie, &clone, "COMMIT");

    // Assert
    assert!(still_active);
    assert_eq!(values(&cassie, &session), vec![vec![Value::Int64(7)]]);
}

#[test]
fn should_retire_the_committed_transaction_through_its_seal() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let mut seal = session.seal_transaction_for_commit();

    // Act
    seal.retire_committed();
    drop(seal);

    // Assert
    assert!(!session.is_transaction_active());
    assert!(!session.is_sealed_for_commit());
}

#[test]
fn should_not_refuse_transaction_control_on_the_committing_thread() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let seal = session.seal_transaction_for_commit();

    // Act
    let own_thread = session.is_committing_on_another_thread();
    let clone = session.clone();
    let other_thread = std::thread::spawn(move || clone.is_committing_on_another_thread())
        .join()
        .unwrap();
    drop(seal);

    // Assert
    assert!(!own_thread);
    assert!(other_thread);
    run(&cassie, &session, "ROLLBACK");
}

#[test]
fn should_ignore_a_direct_rollback_from_another_thread_while_a_commit_is_sealed() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let seal = session.seal_transaction_for_commit();
    let clone = session.clone();

    // Act: pgwire error handling rolls back through the session, not through SQL.
    std::thread::spawn(move || clone.rollback_transaction())
        .join()
        .unwrap();
    let still_sealed = session.is_sealed_for_commit();
    let still_active = session.is_transaction_active();
    drop(seal);

    // Assert
    assert!(still_sealed);
    assert!(still_active);
    run(&cassie, &session, "ROLLBACK");
}

#[test]
fn should_roll_back_on_the_committing_thread_while_sealed() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let seal = session.seal_transaction_for_commit();

    // Act
    session.rollback_transaction();
    drop(seal);

    // Assert
    assert!(!session.is_transaction_active());
    assert!(!session.is_sealed_for_commit());
}

#[test]
fn should_report_a_failed_ready_status_to_other_threads_while_a_commit_is_sealed() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let seal = session.seal_transaction_for_commit();
    let clone = session.clone();

    // Act
    let committing_thread = session.ready_for_query_status();
    let other_thread = std::thread::spawn(move || clone.ready_for_query_status())
        .join()
        .unwrap();
    drop(seal);
    let after_release = session.ready_for_query_status();

    // Assert
    assert_eq!(committing_thread, SessionTransactionStatus::InTransaction);
    assert_eq!(other_thread, SessionTransactionStatus::Failed);
    assert_eq!(after_release, SessionTransactionStatus::InTransaction);
    run(&cassie, &session, "ROLLBACK");
}

#[test]
fn should_record_a_failure_from_another_thread_when_commit_leaves_the_transaction_open() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let seal = session.seal_transaction_for_commit();
    let clone = session.clone();

    // Act
    std::thread::spawn(move || clone.mark_transaction_failed())
        .join()
        .unwrap();
    let failed_while_sealed = session.is_transaction_failed();
    drop(seal);

    // Assert
    assert!(!failed_while_sealed);
    assert!(session.is_transaction_failed());
    run(&cassie, &session, "ROLLBACK");
}

#[test]
fn should_discard_a_deferred_failure_when_commit_retires_the_transaction() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "BEGIN");
    let mut seal = session.seal_transaction_for_commit();
    let clone = session.clone();
    std::thread::spawn(move || clone.mark_transaction_failed())
        .join()
        .unwrap();

    // Act
    seal.retire_committed();
    drop(seal);
    run(&cassie, &session, "BEGIN");

    // Assert
    assert!(!session.is_transaction_failed());
    run(&cassie, &session, "ROLLBACK");
}
