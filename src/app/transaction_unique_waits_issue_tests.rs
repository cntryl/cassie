use std::sync::mpsc;
use std::time::Duration;

use super::observe_next_unique_transaction_wait_for_test;
use crate::app::{Cassie, CassieError, CassieSession, TransactionErrorKind};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

enum WaitEvent {
    ReachedWait,
    Completed(Result<crate::executor::QueryResult, CassieError>),
}

fn fixture(label: &str) -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session(label, None);
    for sql in [
        "CREATE TABLE u (id INT PRIMARY KEY, v INT UNIQUE)",
        "INSERT INTO u VALUES (1, 1), (2, 2)",
    ] {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .expect("fixture SQL");
    }
    (cassie, session)
}

fn start_observed_insert(
    cassie: Cassie,
    session: CassieSession,
    sql: &'static str,
) -> (mpsc::Receiver<WaitEvent>, std::thread::JoinHandle<()>) {
    let (event_sender, event_receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let observed = event_sender.clone();
        let _observer = observe_next_unique_transaction_wait_for_test(move || {
            observed
                .send(WaitEvent::ReachedWait)
                .expect("wait observer receiver");
        });
        let result = cassie.execute_sql(&session, sql, Vec::new());
        event_sender
            .send(WaitEvent::Completed(result))
            .expect("insert result receiver");
    });
    (event_receiver, worker)
}

fn receive_event(events: &mpsc::Receiver<WaitEvent>) -> WaitEvent {
    events
        .recv_timeout(Duration::from_secs(5))
        .expect("unique wait observer or INSERT completion")
}

fn rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, "SELECT id, v FROM u ORDER BY id", Vec::new())
        .expect("read fixture rows")
        .rows()
        .to_vec()
}

#[test]
fn should_make_waiting_insert_report_the_duplicate_after_update_owner_commits() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_commit_owner");
    for sql in ["BEGIN", "UPDATE u SET v = 99 WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage unique value in UPDATE");
    }
    let waiter = cassie.create_session("issue_1465_waiter", None);
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter, "INSERT INTO u VALUES (3, 99)");
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let owner_commit = cassie.execute_sql(&owner, "COMMIT", Vec::new());
    let result_event = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");

    // Assert
    assert!(owner_commit.is_ok(), "UPDATE owner COMMIT must succeed");
    assert!(reached_wait);
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::UniqueViolation { constraint, .. }))
            if constraint == "u_v_key"
    ));
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(1), Value::Int64(99)],
            vec![Value::Int64(2), Value::Int64(2)],
        ]
    );
}

#[test]
fn should_allow_waiting_insert_after_update_owner_rolls_back() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_rollback_owner");
    for sql in ["BEGIN", "UPDATE u SET v = 99 WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage unique value in UPDATE");
    }
    let waiter = cassie.create_session("issue_1465_rollback_waiter", None);
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter, "INSERT INTO u VALUES (3, 99)");
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let owner_rollback = cassie.execute_sql(&owner, "ROLLBACK", Vec::new());
    let result_event = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");

    // Assert
    assert!(owner_rollback.is_ok());
    assert!(reached_wait);
    assert!(matches!(result_event, WaitEvent::Completed(Ok(_))));
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(3), Value::Int64(99)],
        ]
    );
}

#[test]
fn should_fail_explicit_waiter_insert_when_update_owner_commits() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_explicit_waiter");
    for sql in ["BEGIN", "UPDATE u SET v = 99 WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage unique value in UPDATE");
    }
    let waiter = cassie.create_session("issue_1465_explicit_waiter", None);
    cassie
        .execute_sql(&waiter, "BEGIN", Vec::new())
        .expect("begin waiter transaction");
    let (events, worker) = start_observed_insert(
        cassie.clone(),
        waiter.clone(),
        "INSERT INTO u VALUES (3, 99)",
    );
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let owner_commit = cassie.execute_sql(&owner, "COMMIT", Vec::new());
    let result_event = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");
    let waiter_status = waiter.transaction_status();
    let following_statement = cassie.execute_sql(&waiter, "SELECT 1", Vec::new());
    let rollback = cassie.execute_sql(&waiter, "ROLLBACK", Vec::new());

    // Assert
    assert!(owner_commit.is_ok());
    assert!(reached_wait);
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::UniqueViolation { constraint, .. }))
            if constraint == "u_v_key"
    ));
    assert_eq!(waiter_status, "failed");
    assert!(following_statement.is_err());
    assert!(rollback.is_ok());
}

#[test]
fn should_not_wait_for_insert_of_another_value_after_noop_update() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_noop_update");
    for sql in ["BEGIN", "UPDATE u SET v = v WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage no-op UPDATE");
    }
    let insert_session = cassie.create_session("issue_1465_independent_insert", None);
    let (events, worker) = start_observed_insert(
        cassie.clone(),
        insert_session,
        "INSERT INTO u VALUES (3, 50)",
    );

    // Act
    let result_event = receive_event(&events);
    let owner_rollback = cassie.execute_sql(&owner, "ROLLBACK", Vec::new());
    worker.join().expect("independent INSERT worker");

    // Assert
    assert!(matches!(result_event, WaitEvent::Completed(Ok(_))));
    assert!(owner_rollback.is_ok());
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(3), Value::Int64(50)],
        ]
    );
}

#[test]
fn should_detect_deadlock_when_insert_waits_for_update_owner() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_update_wait_cycle");
    for sql in ["BEGIN", "UPDATE u SET v = 99 WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage UPDATE unique value");
    }
    let waiter = cassie.create_session("issue_1465_cycle_waiter", None);
    for sql in ["BEGIN", "INSERT INTO u VALUES (3, 50)"] {
        cassie
            .execute_sql(&waiter, sql, Vec::new())
            .expect("stage second wait-cycle owner");
    }
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter.clone(), "INSERT INTO u VALUES (4, 99)");
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let cycle = cassie.execute_sql(&owner, "INSERT INTO u VALUES (5, 50)", Vec::new());
    let owner_rollback = cassie.execute_sql(&owner, "ROLLBACK", Vec::new());
    let waiting_insert = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");
    let waiter_commit = cassie.execute_sql(&waiter, "COMMIT", Vec::new());

    // Assert
    assert!(matches!(
        cycle,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::DeadlockDetected,
            ..
        })
    ));
    assert!(owner_rollback.is_ok());
    assert!(reached_wait);
    assert!(matches!(waiting_insert, WaitEvent::Completed(Ok(_))));
    assert!(waiter_commit.is_ok());
}
