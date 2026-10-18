//! Embedded `PostgreSQL` 18 transaction-error identities and failed-block recovery.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::{CassieError, TransactionErrorKind};
use cassie::types::Value;
use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

const ABORTED: &str =
    "current transaction is aborted, commands ignored until end of transaction block";

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &["CREATE TABLE tx_error_items (id INT PRIMARY KEY, label TEXT)"],
    )
}

fn transaction_error(error: &CassieError) -> Option<(TransactionErrorKind, String)> {
    match error {
        CassieError::Transaction { kind, message } => Some((*kind, message.clone())),
        _ => None,
    }
}

fn fail_block(fixture: &SqlFixture) {
    for sql in [
        "BEGIN",
        "INSERT INTO tx_error_items (id, label) VALUES (1, 'kept')",
    ] {
        assert!(fixture.execute(sql).is_ok(), "setup failed: {sql}");
    }
    let duplicate = fixture.execute("INSERT INTO tx_error_items (id, label) VALUES (1, 'dup')");
    assert!(duplicate.is_err(), "duplicate key must fail the block");
}

fn committed_ids(fixture: &SqlFixture) -> Vec<Vec<Value>> {
    fixture.rows("SELECT id FROM tx_error_items ORDER BY id")
}

#[test]
fn should_reject_ordinary_statements_in_a_failed_transaction_block() {
    // Arrange
    let fixture = fixture("tx-error-failed-block");
    fail_block(&fixture);

    // Act
    let errors = [
        "SELECT id FROM tx_error_items",
        "INSERT INTO tx_error_items (id, label) VALUES (2, 'ignored')",
        "SAVEPOINT late",
        "RELEASE SAVEPOINT late",
        "BEGIN",
    ]
    .map(|sql| transaction_error(&fixture.error(sql)));

    // Assert
    for error in errors {
        assert_eq!(
            error,
            Some((TransactionErrorKind::FailedTransaction, ABORTED.to_string()))
        );
    }
    assert_eq!(fixture.session.transaction_status(), "failed");
}

#[test]
fn should_roll_back_a_failed_transaction_block_on_commit() {
    // Arrange
    let fixture = fixture("tx-error-failed-commit");
    fail_block(&fixture);

    // Act
    let committed = fixture.execute("COMMIT");

    // Assert
    let Ok(committed) = committed else {
        panic!("COMMIT of a failed block must complete as ROLLBACK");
    };
    assert_eq!(committed.command(), "ROLLBACK");
    assert_eq!(fixture.session.transaction_status(), "idle");
    assert_eq!(committed_ids(&fixture), Vec::<Vec<Value>>::new());
}

#[test]
fn should_reject_missing_savepoints_as_invalid_savepoint() {
    // Arrange
    let fixture = fixture("tx-error-missing-savepoint");

    // Act
    let mut errors = Vec::new();
    for control in ["ROLLBACK TO SAVEPOINT missing", "RELEASE SAVEPOINT missing"] {
        assert!(fixture.execute("BEGIN").is_ok(), "BEGIN must open a block");
        errors.push(transaction_error(&fixture.error(control)));
        errors.push(transaction_error(&fixture.error("SELECT 1")));
        assert!(fixture.execute("ROLLBACK").is_ok(), "ROLLBACK must recover");
    }
    fail_block(&fixture);
    let failed_missing = transaction_error(&fixture.error("ROLLBACK TO SAVEPOINT missing"));
    let still_failed = fixture.session.transaction_status();

    // Assert
    let invalid = Some((
        TransactionErrorKind::InvalidSavepoint,
        "savepoint \"missing\" does not exist".to_string(),
    ));
    let aborted = Some((TransactionErrorKind::FailedTransaction, ABORTED.to_string()));
    assert_eq!(
        errors,
        vec![invalid.clone(), aborted.clone(), invalid.clone(), aborted]
    );
    assert_eq!(failed_missing, invalid);
    assert_eq!(still_failed, "failed");
}

#[test]
fn should_reject_savepoint_controls_outside_a_transaction_block() {
    // Arrange
    let fixture = fixture("tx-error-idle-savepoint");

    // Act
    let errors = [
        "SAVEPOINT idle_point",
        "ROLLBACK TO SAVEPOINT idle_point",
        "RELEASE SAVEPOINT idle_point",
    ]
    .map(|sql| transaction_error(&fixture.error(sql)));

    // Assert
    assert_eq!(
        errors,
        [
            "SAVEPOINT can only be used in transaction blocks",
            "ROLLBACK TO SAVEPOINT can only be used in transaction blocks",
            "RELEASE SAVEPOINT can only be used in transaction blocks",
        ]
        .map(|message| Some((
            TransactionErrorKind::NoActiveTransaction,
            message.to_string()
        )))
    );
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_recover_a_failed_transaction_block_through_an_existing_savepoint() {
    // Arrange
    let fixture = fixture("tx-error-savepoint-recovery");
    for sql in [
        "BEGIN",
        "INSERT INTO tx_error_items (id, label) VALUES (1, 'kept')",
        "SAVEPOINT before_dup",
    ] {
        assert!(fixture.execute(sql).is_ok(), "setup failed: {sql}");
    }
    let duplicate = fixture.execute("INSERT INTO tx_error_items (id, label) VALUES (1, 'dup')");
    assert!(duplicate.is_err(), "duplicate key must fail the block");

    // Act
    let recovered = fixture.execute("ROLLBACK TO SAVEPOINT before_dup");
    let inserted = fixture.execute("INSERT INTO tx_error_items (id, label) VALUES (2, 'after')");
    let committed = fixture.execute("COMMIT");

    // Assert
    assert!(
        recovered.is_ok(),
        "existing savepoint must recover the block"
    );
    assert!(
        inserted.is_ok(),
        "recovered block must accept ordinary work"
    );
    let Ok(committed) = committed else {
        panic!("recovered block must commit");
    };
    assert_eq!(committed.command(), "COMMIT");
    assert_eq!(fixture.session.transaction_status(), "idle");
    assert_eq!(
        committed_ids(&fixture),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_restore_session_settings_when_commit_rolls_back_a_failed_block() {
    // Arrange
    let fixture = fixture("tx-error-failed-commit-settings");
    let original = fixture.session.search_path();
    for sql in ["BEGIN", "SET search_path TO tx_error_other"] {
        assert!(fixture.execute(sql).is_ok(), "setup failed: {sql}");
    }
    let failed = fixture.execute("SELECT 1 / 0");
    assert!(failed.is_err(), "division by zero must fail the block");

    // Act
    let committed = fixture.execute("COMMIT");

    // Assert
    assert!(committed.is_ok(), "COMMIT of a failed block must complete");
    assert_eq!(fixture.session.search_path(), original);
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_wait_for_a_unique_conflict_owner_before_returning_insert() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-wait",
        &[
            "CREATE TABLE tx_unique_wait (id INT PRIMARY KEY, label TEXT UNIQUE)",
            "BEGIN",
            "INSERT INTO tx_unique_wait (id, label) VALUES (1, 'same') RETURNING id",
        ],
    );
    let owner_session = fixture.session.clone();
    let waiter_session = fixture.cassie.create_session("waiter", None);
    let cassie = fixture.cassie.clone();
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the conflicting transaction");

    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let thread_session = waiter_session.clone();
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &thread_session,
            "INSERT INTO tx_unique_wait (id, label) VALUES (2, 'same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let before_owner_commit = result_receiver.recv_timeout(Duration::from_millis(250));
    let returned_before_owner_commit = before_owner_commit.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&owner_session, "COMMIT", vec![])
        .expect("resolve the unique-key owner transaction");
    let waiter_result = match before_owner_commit {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("conflicting insert completes after owner resolution"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("conflicting insert worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("conflicting insert worker should finish");
    let waiter_state = waiter_session.transaction_status();
    fixture
        .cassie
        .execute_sql(&waiter_session, "ROLLBACK", vec![])
        .expect("recover the failed conflicting transaction");

    // Assert
    assert!(
        !returned_before_owner_commit,
        "PostgreSQL waits for the unresolved unique-key owner before returning INSERT"
    );
    assert!(
        matches!(waiter_result, Err(CassieError::UniqueViolation { .. })),
        "committing the owner first makes the waiting INSERT fail with 23505"
    );
    assert_eq!(waiter_state, "failed");
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait ORDER BY id"),
        vec![vec![Value::Int64(1)]]
    );
}

#[test]
fn should_publish_transactional_table_creation_only_after_commit() {
    // Arrange
    let fixture = fixture("tx-ddl-visibility");
    let observer = fixture.cassie.create_session("observer", None);
    fixture.execute("BEGIN").expect("begin transaction");

    // Act
    fixture
        .execute("CREATE TABLE tx_ddl_pending (id INT PRIMARY KEY, label TEXT)")
        .expect("create table transactionally");
    fixture
        .execute("INSERT INTO tx_ddl_pending (id, label) VALUES (1, 'private')")
        .expect("write through uncommitted table");
    let private = fixture
        .execute("SELECT id, label FROM tx_ddl_pending")
        .expect("read own transactional DDL");
    let invisible =
        fixture
            .cassie
            .execute_sql(&observer, "SELECT id, label FROM tx_ddl_pending", vec![]);
    fixture
        .execute("COMMIT")
        .expect("publish transactional DDL");
    let published = fixture
        .cassie
        .execute_sql(&observer, "SELECT id, label FROM tx_ddl_pending", vec![])
        .expect("read the committed table from another session");

    // Assert
    assert_eq!(
        private.rows(),
        vec![vec![Value::Int64(1), Value::String("private".into())]]
    );
    assert!(invisible.is_err(), "uncommitted table must not be visible");
    assert_eq!(
        published.rows(),
        vec![vec![Value::Int64(1), Value::String("private".into())]]
    );
}

#[test]
fn should_restore_transactional_table_alteration_after_rollback() {
    // Arrange
    let fixture = fixture("tx-ddl-rollback");
    fixture.execute("BEGIN").expect("begin transaction");

    // Act
    fixture
        .execute("ALTER TABLE tx_error_items ADD COLUMN transient TEXT")
        .expect("alter table transactionally");
    fixture.execute("ROLLBACK").expect("rollback DDL");
    let absent = fixture.execute("SELECT transient FROM tx_error_items");

    // Assert
    assert!(
        absent.is_err(),
        "rolled-back column must not remain in schema"
    );
    assert_eq!(fixture.session.transaction_status(), "idle");
}
