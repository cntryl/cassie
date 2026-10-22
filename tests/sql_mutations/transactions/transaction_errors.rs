//! Embedded `PostgreSQL` 18 transaction-error identities and failed-block recovery.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::{CassieError, TransactionErrorKind};
use cassie::types::Value;
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

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

fn wait_for_running_query(cassie: &cassie::app::Cassie) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if cassie.metrics()["runtime"]["running_queries"]
            .as_u64()
            .is_some_and(|running| running > 0)
        {
            return true;
        }
        std::thread::yield_now();
    }
    false
}

fn begin_and_stage_unique_row(
    cassie: &cassie::app::Cassie,
    session: &cassie::app::CassieSession,
    insert: &str,
) {
    for sql in ["BEGIN", insert] {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("stage an owned unique value");
    }
}

fn start_waiting_multi_row_insert(
    cassie: cassie::app::Cassie,
    session: cassie::app::CassieSession,
) -> (
    std::thread::JoinHandle<()>,
    mpsc::Receiver<Result<cassie::executor::QueryResult, CassieError>>,
) {
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &session,
            "INSERT INTO tx_unique_wait_multiple (id, label) VALUES (3, 'first'), (4, 'second') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the waiting multi-row INSERT result");
    });
    started.wait();
    (waiter, result_receiver)
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
            "CREATE TABLE tx_unique_wait (id SERIAL PRIMARY KEY, label TEXT UNIQUE)",
            "BEGIN",
            "INSERT INTO tx_unique_wait (label) VALUES ('same') RETURNING id",
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
            "INSERT INTO tx_unique_wait (label) VALUES ('same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_owner_commit = result_receiver.recv_timeout(Duration::from_millis(500));
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
        request_entered_engine,
        "the conflicting INSERT must enter the engine before owner commit"
    );
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
    assert_eq!(
        fixture.rows("INSERT INTO tx_unique_wait (label) VALUES ('next') RETURNING id"),
        vec![vec![Value::Int64(3)]],
        "the waiting INSERT must not evaluate its sequence-backed default twice"
    );
}

#[test]
fn should_wait_for_a_unique_conflict_owner_before_returning_insert_when_owner_rolls_back() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-wait-rollback",
        &[
            "CREATE TABLE tx_unique_wait_rollback (id SERIAL PRIMARY KEY, label TEXT UNIQUE)",
            "BEGIN",
            "INSERT INTO tx_unique_wait_rollback (label) VALUES ('same') RETURNING id",
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
            "INSERT INTO tx_unique_wait_rollback (label) VALUES ('same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_owner_rollback = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_owner_rollback = before_owner_rollback.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&owner_session, "ROLLBACK", vec![])
        .expect("release the unique-key owner transaction");
    let waiter_result = match before_owner_rollback {
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
        .execute_sql(&waiter_session, "COMMIT", vec![])
        .expect("commit the insert after the owner rolled back");

    // Assert
    assert!(
        request_entered_engine,
        "the conflicting INSERT must enter the engine before owner rollback"
    );
    assert!(
        !returned_before_owner_rollback,
        "PostgreSQL waits for the unresolved unique-key owner before returning INSERT"
    );
    assert_eq!(
        waiter_result
            .expect("the rolled-back owner frees the key")
            .rows(),
        vec![vec![Value::Int64(2)]],
        "waiting must not evaluate the sequence-backed default twice"
    );
    assert_eq!(waiter_state, "in_transaction");
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_rollback ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_wait_for_each_unique_owner_in_a_multi_row_insert() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-multiple-owners",
        &["CREATE TABLE tx_unique_wait_multiple (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let first_owner = fixture.session.clone();
    begin_and_stage_unique_row(
        &fixture.cassie,
        &first_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (1, 'first')",
    );
    let second_owner = fixture.cassie.create_session("second-owner", None);
    begin_and_stage_unique_row(
        &fixture.cassie,
        &second_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (2, 'second')",
    );
    let waiter_session = fixture.cassie.create_session("waiter", None);
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the multi-row waiting transaction");
    let (waiter, result_receiver) =
        start_waiting_multi_row_insert(fixture.cassie.clone(), waiter_session.clone());
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_first_owner_rollback = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_first_owner_rollback = before_first_owner_rollback.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&first_owner, "ROLLBACK", vec![])
        .expect("release the first unique-key owner");
    let after_first_owner_rollback = if let Ok(result) = before_first_owner_rollback {
        Some(result)
    } else {
        match result_receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(result) => Some(result),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("multi-row INSERT worker exited without returning a result")
            }
        }
    };
    let returned_while_second_owner_holds_key = after_first_owner_rollback.is_some();
    fixture
        .cassie
        .execute_sql(&second_owner, "ROLLBACK", vec![])
        .expect("release the second unique-key owner");
    let waiter_result = after_first_owner_rollback.unwrap_or_else(|| {
        result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("multi-row INSERT completes after both owners roll back")
    });
    waiter
        .join()
        .expect("multi-row INSERT worker should finish");
    let transaction_resolution = if waiter_result.is_ok() {
        "COMMIT"
    } else {
        "ROLLBACK"
    };
    fixture
        .cassie
        .execute_sql(&waiter_session, transaction_resolution, vec![])
        .expect("resolve the multi-row waiting transaction");

    // Assert
    assert!(
        request_entered_engine,
        "multi-row INSERT must enter the engine"
    );
    assert!(
        !returned_before_first_owner_rollback,
        "multi-row INSERT must wait for the first unique-key owner"
    );
    assert!(
        !returned_while_second_owner_holds_key,
        "resolving one key must leave the INSERT waiting for its other owner"
    );
    assert_eq!(
        waiter_result
            .expect("both owner rollbacks release their unique values")
            .rows(),
        vec![vec![Value::Int64(3)], vec![Value::Int64(4)]]
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_multiple ORDER BY id"),
        vec![vec![Value::Int64(3)], vec![Value::Int64(4)]]
    );
}

#[test]
fn should_report_a_committed_conflict_before_waiting_on_other_insert_keys() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-multi-owner-commit",
        &["CREATE TABLE tx_unique_wait_multiple (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let first_owner = fixture.session.clone();
    begin_and_stage_unique_row(
        &fixture.cassie,
        &first_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (1, 'first')",
    );
    let second_owner = fixture.cassie.create_session("second-owner", None);
    begin_and_stage_unique_row(
        &fixture.cassie,
        &second_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (2, 'second')",
    );
    let waiter_session = fixture.cassie.create_session("waiter", None);
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the multi-row waiting transaction");
    let (waiter, result_receiver) =
        start_waiting_multi_row_insert(fixture.cassie.clone(), waiter_session.clone());
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_first_owner_commit = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_first_owner_commit = before_first_owner_commit.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&first_owner, "COMMIT", vec![])
        .expect("commit the owner of the first INSERT key");
    let after_first_owner_commit = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_second_owner_resolution = after_first_owner_commit.is_ok();
    fixture
        .cassie
        .execute_sql(&second_owner, "ROLLBACK", vec![])
        .expect("release the other unique-key owner");
    let waiter_result = match after_first_owner_commit {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("multi-row INSERT completes after remaining owner resolution"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("multi-row INSERT worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("multi-row INSERT worker should finish");
    fixture
        .cassie
        .execute_sql(&waiter_session, "ROLLBACK", vec![])
        .expect("roll back the failed multi-row INSERT transaction");

    // Assert
    assert!(
        request_entered_engine,
        "multi-row INSERT must enter the engine"
    );
    assert!(
        !returned_before_first_owner_commit,
        "the multi-row INSERT must wait for the first owner"
    );
    assert!(
        returned_before_second_owner_resolution,
        "a committed duplicate must fail the INSERT without waiting for another key owner"
    );
    assert!(
        matches!(waiter_result, Err(CassieError::UniqueViolation { .. })),
        "the committed owner must produce a unique violation"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_multiple ORDER BY id"),
        vec![vec![Value::Int64(1)]]
    );
}

#[test]
fn should_release_unique_key_waiters_when_owner_rolls_back_to_savepoint() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-savepoint",
        &[
            "CREATE TABLE tx_unique_wait_savepoint (id INT PRIMARY KEY, label TEXT UNIQUE)",
            "BEGIN",
            "SAVEPOINT before_insert",
            "INSERT INTO tx_unique_wait_savepoint (id, label) VALUES (1, 'same') RETURNING id",
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
            "INSERT INTO tx_unique_wait_savepoint (id, label) VALUES (2, 'same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_savepoint_rollback = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_savepoint_rollback = before_savepoint_rollback.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(
            &owner_session,
            "ROLLBACK TO SAVEPOINT before_insert",
            vec![],
        )
        .expect("release the staged key while keeping the owner transaction open");
    let waiter_result = match before_savepoint_rollback {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("conflicting insert completes after savepoint rollback"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("conflicting insert worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("conflicting insert worker should finish");
    fixture
        .cassie
        .execute_sql(&owner_session, "COMMIT", vec![])
        .expect("commit the transaction after rolling back to savepoint");
    fixture
        .cassie
        .execute_sql(&waiter_session, "COMMIT", vec![])
        .expect("commit the insert after the owner rolled back to savepoint");

    // Assert
    assert!(
        request_entered_engine,
        "the conflicting INSERT must enter the engine before savepoint rollback"
    );
    assert!(
        !returned_before_savepoint_rollback,
        "PostgreSQL waits for the unresolved unique-key owner until savepoint rollback"
    );
    assert!(
        waiter_result.is_ok(),
        "savepoint rollback frees the staged key"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_savepoint ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_release_unique_key_waiters_when_owner_session_drops() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-session-drop",
        &["CREATE TABLE tx_unique_wait_drop (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let owner_session = fixture.cassie.create_session("owner", None);
    for sql in [
        "BEGIN",
        "INSERT INTO tx_unique_wait_drop (id, label) VALUES (1, 'same')",
    ] {
        fixture
            .cassie
            .execute_sql(&owner_session, sql, vec![])
            .expect("stage the unique key for the owner");
    }
    let waiter_session = fixture.cassie.create_session("waiter", None);
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the waiting transaction");
    let cassie = fixture.cassie.clone();
    let thread_session = waiter_session.clone();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &thread_session,
            "INSERT INTO tx_unique_wait_drop (id, label) VALUES (2, 'same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_owner_drop = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_owner_drop = before_owner_drop.is_ok();

    // Act
    drop(owner_session);
    let waiter_result = match before_owner_drop {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("waiting insert completes after owner session drop"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("conflicting insert worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("conflicting insert worker should finish");
    fixture
        .cassie
        .execute_sql(&waiter_session, "COMMIT", vec![])
        .expect("commit the insert after the owner session drops");

    // Assert
    assert!(request_entered_engine, "the INSERT must enter the engine");
    assert!(
        !returned_before_owner_drop,
        "PostgreSQL waits until the transaction owner exits"
    );
    assert!(waiter_result.is_ok(), "dropping the owner releases its key");
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_drop ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_keep_deadlock_victim_keys_until_transaction_resolution() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-deadlock-savepoint",
        &["CREATE TABLE tx_unique_wait_deadlock (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let cycle = start_deadlock_wait_cycle(&fixture);
    let before_cycle = cycle
        .result_receiver
        .recv_timeout(Duration::from_millis(500));
    assert!(
        matches!(before_cycle, Err(mpsc::RecvTimeoutError::Timeout)),
        "the first insert must be waiting on the second transaction before closing the cycle"
    );
    // Act
    let deadlock_result = fixture.cassie.execute_sql(
        &cycle.victim_session,
        "INSERT INTO tx_unique_wait_deadlock VALUES (4, 'first')",
        vec![],
    );
    let before_savepoint_rollback = cycle
        .result_receiver
        .recv_timeout(Duration::from_millis(500));
    let savepoint_result = fixture.cassie.execute_sql(
        &cycle.victim_session,
        "ROLLBACK TO SAVEPOINT after_key",
        vec![],
    );
    let (waiter_returned_before_savepoint_rollback, before_owner_commit) =
        resolve_waiter_after_savepoint(&cycle.result_receiver, before_savepoint_rollback);
    let waiter_returned_before_owner_commit = before_owner_commit.is_some();
    let owner_commit_result = fixture
        .cassie
        .execute_sql(&cycle.victim_session, "COMMIT", vec![]);
    if owner_commit_result.is_err() {
        fixture
            .cassie
            .execute_sql(&cycle.victim_session, "ROLLBACK", vec![])
            .expect("clean up the deadlock victim transaction");
    }
    let waiter_result = before_owner_commit.unwrap_or_else(|| {
        cycle
            .result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("waiting insert completes after owner transaction resolution")
    });
    cycle
        .waiter
        .join()
        .expect("waiting insert worker should finish");
    let first_resolution = if waiter_result.is_ok() {
        "COMMIT"
    } else {
        "ROLLBACK"
    };
    fixture
        .cassie
        .execute_sql(&cycle.first_session, first_resolution, vec![])
        .expect("resolve the first transaction after the waiting insert completes");
    // Assert
    assert!(
        cycle.request_entered_engine,
        "the first insert must enter the engine"
    );
    assert!(
        matches!(
            deadlock_result,
            Err(CassieError::Transaction {
                kind: TransactionErrorKind::DeadlockDetected,
                ..
            })
        ),
        "the second conflicting insert must detect the wait cycle"
    );
    assert!(
        !waiter_returned_before_savepoint_rollback,
        "deadlock detection must preserve keys owned before the failed statement"
    );
    assert!(
        savepoint_result.is_ok()
            && !waiter_returned_before_owner_commit
            && owner_commit_result.is_ok(),
        "savepoint recovery must keep the victim key until owner commit"
    );
    assert!(
        matches!(waiter_result, Err(CassieError::UniqueViolation { .. })),
        "the waiting INSERT fails after the recovered owner commits"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_deadlock ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

fn resolve_waiter_after_savepoint(
    receiver: &mpsc::Receiver<Result<cassie::executor::QueryResult, CassieError>>,
    before_savepoint: Result<
        Result<cassie::executor::QueryResult, CassieError>,
        mpsc::RecvTimeoutError,
    >,
) -> (
    bool,
    Option<Result<cassie::executor::QueryResult, CassieError>>,
) {
    let returned_before_savepoint = before_savepoint.is_ok();
    let before_owner_commit = if let Ok(result) = before_savepoint {
        Some(result)
    } else {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(result) => Some(result),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("waiting insert worker exited without returning a result")
            }
        }
    };

    (returned_before_savepoint, before_owner_commit)
}

struct DeadlockWaitCycle {
    first_session: cassie::app::CassieSession,
    victim_session: cassie::app::CassieSession,
    waiter: std::thread::JoinHandle<()>,
    result_receiver: mpsc::Receiver<Result<cassie::executor::QueryResult, CassieError>>,
    request_entered_engine: bool,
}

fn start_deadlock_wait_cycle(fixture: &SqlFixture) -> DeadlockWaitCycle {
    let first_session = fixture.session.clone();
    let victim_session = fixture.cassie.create_session("second", None);
    for (session, statements) in [
        (
            &first_session,
            [
                "BEGIN",
                "INSERT INTO tx_unique_wait_deadlock VALUES (1, 'first')",
                "SAVEPOINT after_key",
            ],
        ),
        (
            &victim_session,
            [
                "BEGIN",
                "INSERT INTO tx_unique_wait_deadlock VALUES (2, 'second')",
                "SAVEPOINT after_key",
            ],
        ),
    ] {
        for sql in statements {
            fixture
                .cassie
                .execute_sql(session, sql, vec![])
                .expect("stage a unique key for each transaction");
        }
    }

    let cassie = fixture.cassie.clone();
    let thread_session = first_session.clone();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &thread_session,
            "INSERT INTO tx_unique_wait_deadlock VALUES (3, 'second') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the waiting insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);

    DeadlockWaitCycle {
        first_session,
        victim_session,
        waiter,
        result_receiver,
        request_entered_engine,
    }
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
