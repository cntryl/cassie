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
    let waiter_owner = waiter.transaction_unique_owner_id();
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
    assert!(cassie
        .transaction_unique_waits
        .snapshot_owner_keys(waiter_owner)
        .is_empty());
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
fn should_not_resume_autocommit_insert_in_a_replacement_transaction() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_replaced_waiter_transaction");
    for sql in ["BEGIN", "UPDATE u SET v = 99 WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage unique value in UPDATE");
    }
    let waiter = cassie.create_session("issue_1465_replaced_waiter", None);
    let initial_owner = waiter.transaction_unique_owner_id();
    let (events, worker) = start_observed_insert(
        cassie.clone(),
        waiter.clone(),
        "INSERT INTO u VALUES (3, 99)",
    );
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let begin_replacement = cassie.execute_sql(&waiter, "BEGIN", Vec::new());
    let replacement_owner = waiter.transaction_unique_owner_id();
    let owner_rollback = cassie.execute_sql(&owner, "ROLLBACK", Vec::new());
    let result_event = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");
    let visible_rows = rows(&cassie, &waiter);
    let replacement_rollback = cassie.execute_sql(&waiter, "ROLLBACK", Vec::new());

    // Assert
    assert!(reached_wait);
    assert!(begin_replacement.is_ok());
    assert_ne!(replacement_owner, initial_owner);
    assert!(owner_rollback.is_ok());
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::Execution(_)))
    ));
    assert_eq!(waiter.transaction_status(), "in_transaction");
    assert_eq!(
        visible_rows,
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
        ]
    );
    assert!(replacement_rollback.is_ok());
}

#[test]
fn should_wait_for_autocommit_insert_while_delete_owner_is_unresolved() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_delete_owner");
    for sql in ["BEGIN", "DELETE FROM u WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage DELETE of unique value");
    }
    let waiter = cassie.create_session("issue_1465_delete_waiter", None);
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter, "INSERT INTO u VALUES (3, 1)");
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
    assert!(reached_wait);
    assert!(owner_commit.is_ok());
    assert!(matches!(result_event, WaitEvent::Completed(Ok(_))));
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(3), Value::Int64(1)],
        ]
    );
}

#[test]
fn should_recheck_unique_value_after_delete_owner_rolls_back() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_delete_owner_rollback");
    for sql in ["BEGIN", "DELETE FROM u WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage DELETE of unique value");
    }
    let waiter = cassie.create_session("issue_1465_delete_rollback_waiter", None);
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter, "INSERT INTO u VALUES (3, 1)");
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
    assert!(reached_wait);
    assert!(owner_rollback.is_ok());
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::UniqueViolation {
            constraint,
            ..
        })) if constraint == "u_v_key"
    ));
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
        ]
    );
}

#[test]
fn should_wait_for_primary_key_reuse_while_delete_owner_is_unresolved() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_primary_key_delete_owner");
    for sql in ["BEGIN", "DELETE FROM u WHERE id = 1"] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage DELETE of primary key");
    }
    let waiter = cassie.create_session("issue_1465_primary_key_waiter", None);
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter, "INSERT INTO u VALUES (1, 50)");
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
    assert!(reached_wait);
    assert!(owner_commit.is_ok());
    assert!(matches!(result_event, WaitEvent::Completed(Ok(_))));
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(1), Value::Int64(50)],
            vec![Value::Int64(2), Value::Int64(2)],
        ]
    );
}

#[test]
fn should_wait_for_autocommit_insert_while_merge_updates_unique_value() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_merge_update_owner");
    for sql in [
        "CREATE TABLE merge_source (id INT PRIMARY KEY, v INT)",
        "INSERT INTO merge_source VALUES (1, 99)",
        "BEGIN",
        "MERGE INTO u AS target USING merge_source AS source ON target.id = source.id WHEN MATCHED THEN UPDATE SET v = source.v",
    ] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage MERGE unique value");
    }
    let waiter = cassie.create_session("issue_1465_merge_update_waiter", None);
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
    assert!(reached_wait);
    assert!(owner_commit.is_ok());
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::UniqueViolation {
            constraint,
            ..
        })) if constraint == "u_v_key"
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
fn should_allow_waiting_insert_after_merge_update_owner_rolls_back() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_merge_update_rollback");
    for sql in [
        "CREATE TABLE merge_source (id INT PRIMARY KEY, v INT)",
        "INSERT INTO merge_source VALUES (1, 99)",
        "BEGIN",
        "MERGE INTO u AS target USING merge_source AS source ON target.id = source.id WHEN MATCHED THEN UPDATE SET v = source.v",
    ] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage MERGE unique value");
    }
    let waiter = cassie.create_session("issue_1465_merge_update_rollback_waiter", None);
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
    assert!(reached_wait);
    assert!(owner_rollback.is_ok());
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
fn should_wait_for_autocommit_insert_while_merge_delete_owner_is_unresolved() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_merge_delete_owner");
    for sql in [
        "CREATE TABLE merge_source (id INT PRIMARY KEY)",
        "INSERT INTO merge_source VALUES (1)",
        "BEGIN",
        "MERGE INTO u AS target USING merge_source AS source ON target.id = source.id WHEN MATCHED THEN DELETE",
    ] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage MERGE DELETE of unique value");
    }
    let waiter = cassie.create_session("issue_1465_merge_delete_waiter", None);
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter, "INSERT INTO u VALUES (3, 1)");
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
    assert!(reached_wait);
    assert!(owner_commit.is_ok());
    assert!(matches!(result_event, WaitEvent::Completed(Ok(_))));
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(3), Value::Int64(1)],
        ]
    );
}

#[test]
fn should_recheck_unique_value_after_merge_delete_owner_rolls_back() {
    // Arrange
    let (cassie, owner) = fixture("issue_1465_merge_delete_rollback");
    for sql in [
        "CREATE TABLE merge_source (id INT PRIMARY KEY)",
        "INSERT INTO merge_source VALUES (1)",
        "BEGIN",
        "MERGE INTO u AS target USING merge_source AS source ON target.id = source.id WHEN MATCHED THEN DELETE",
    ] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage MERGE DELETE of unique value");
    }
    let waiter = cassie.create_session("issue_1465_merge_delete_rollback_waiter", None);
    let (events, worker) =
        start_observed_insert(cassie.clone(), waiter, "INSERT INTO u VALUES (3, 1)");
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
    assert!(reached_wait);
    assert!(owner_rollback.is_ok());
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::UniqueViolation {
            constraint,
            ..
        })) if constraint == "u_v_key"
    ));
    assert_eq!(
        rows(&cassie, &owner),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
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
    let (events, worker) = start_observed_insert(
        cassie.clone(),
        waiter.clone(),
        "INSERT INTO u VALUES (4, 99)",
    );
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

fn two_key_fixture(label: &str) -> (Cassie, CassieSession, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let first = cassie.create_session(label, None);
    cassie
        .execute_sql(
            &first,
            "CREATE TABLE t (a INT, b INT, CONSTRAINT t_a_key UNIQUE (a), CONSTRAINT t_b_key UNIQUE (b))",
            Vec::new(),
        )
        .expect("create two-key table");
    let second = cassie.create_session(&format!("{label}_second"), None);
    (cassie, first, second)
}

fn two_key_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, "SELECT a, b FROM t ORDER BY a", Vec::new())
        .expect("read two-key fixture rows")
        .rows()
        .to_vec()
}

fn start_two_key_waiter(
    cassie: &Cassie,
    waiter: CassieSession,
) -> (mpsc::Receiver<WaitEvent>, std::thread::JoinHandle<()>) {
    start_observed_insert(cassie.clone(), waiter, "INSERT INTO t VALUES (1, 2)")
}

#[test]
fn should_recheck_other_unique_keys_after_waiter_owner_rolls_back() {
    // Arrange
    let (cassie, first, second) = two_key_fixture("issue_1466_wait_owner_rollback");
    for sql in ["BEGIN", "INSERT INTO t VALUES (1, 100)"] {
        cassie
            .execute_sql(&first, sql, Vec::new())
            .expect("stage first key");
    }
    let waiter = cassie.create_session("issue_1466_waiter", None);
    cassie
        .execute_sql(&waiter, "BEGIN", Vec::new())
        .expect("begin waiter transaction");
    let (events, worker) = start_two_key_waiter(&cassie, waiter.clone());
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let other_commit = reached_wait.then(|| {
        ["BEGIN", "INSERT INTO t VALUES (9, 2)", "COMMIT"]
            .into_iter()
            .try_for_each(|sql| cassie.execute_sql(&second, sql, Vec::new()).map(|_| ()))
    });
    if matches!(&other_commit, Some(Err(_))) {
        let _ = cassie.execute_sql(&second, "ROLLBACK", Vec::new());
    }
    let owner_rollback = cassie.execute_sql(&first, "ROLLBACK", Vec::new());
    let result_event = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");
    let status = waiter.transaction_status();
    let following_statement = cassie.execute_sql(&waiter, "SELECT 1", Vec::new());
    let rollback = cassie.execute_sql(&waiter, "ROLLBACK", Vec::new());
    let committed_rows = two_key_rows(&cassie, &second);

    // Assert
    assert!(owner_rollback.is_ok());
    assert!(reached_wait);
    other_commit
        .expect("commit was attempted after the INSERT reached its wait")
        .expect("commit the other unique key");
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::UniqueViolation {
            constraint,
            ..
        })) if constraint == "t_b_key"
    ));
    assert_eq!(status, "failed");
    assert!(following_statement.is_err());
    assert!(rollback.is_ok());
    assert_eq!(committed_rows, vec![vec![Value::Int64(9), Value::Int64(2)]]);
}

#[test]
fn should_report_waited_unique_key_conflict_after_owner_commits() {
    // Arrange
    let (cassie, first, _) = two_key_fixture("issue_1466_wait_owner_commit");
    for sql in ["BEGIN", "INSERT INTO t VALUES (1, 100)"] {
        cassie
            .execute_sql(&first, sql, Vec::new())
            .expect("stage owner row");
    }
    let waiter = cassie.create_session("issue_1466_waiter", None);
    cassie
        .execute_sql(&waiter, "BEGIN", Vec::new())
        .expect("begin waiter transaction");
    let (events, worker) = start_two_key_waiter(&cassie, waiter.clone());
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let owner_commit = cassie.execute_sql(&first, "COMMIT", Vec::new());
    let result_event = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");
    let rollback = cassie.execute_sql(&waiter, "ROLLBACK", Vec::new());

    // Assert
    assert!(owner_commit.is_ok());
    assert!(reached_wait);
    assert!(matches!(
        result_event,
        WaitEvent::Completed(Err(CassieError::UniqueViolation {
            constraint,
            ..
        })) if constraint == "t_a_key"
    ));
    assert!(rollback.is_ok());
}

#[test]
fn should_allow_multi_key_insert_after_wait_owner_rolls_back_without_other_conflicts() {
    // Arrange
    let (cassie, first, _) = two_key_fixture("issue_1466_no_other_conflict");
    for sql in ["BEGIN", "INSERT INTO t VALUES (1, 100)"] {
        cassie
            .execute_sql(&first, sql, Vec::new())
            .expect("stage owner row");
    }
    let waiter = cassie.create_session("issue_1466_waiter", None);
    cassie
        .execute_sql(&waiter, "BEGIN", Vec::new())
        .expect("begin waiter transaction");
    let (events, worker) = start_two_key_waiter(&cassie, waiter.clone());
    let wait_event = receive_event(&events);
    let reached_wait = matches!(&wait_event, WaitEvent::ReachedWait);

    // Act
    let owner_rollback = cassie.execute_sql(&first, "ROLLBACK", Vec::new());
    let result_event = if reached_wait {
        receive_event(&events)
    } else {
        wait_event
    };
    worker.join().expect("waiting INSERT worker");
    let commit = cassie.execute_sql(&waiter, "COMMIT", Vec::new());
    let committed_rows = two_key_rows(&cassie, &waiter);

    // Assert
    assert!(owner_rollback.is_ok());
    assert!(reached_wait);
    assert!(matches!(result_event, WaitEvent::Completed(Ok(_))));
    assert!(commit.is_ok());
    assert_eq!(committed_rows, vec![vec![Value::Int64(1), Value::Int64(2)]]);
}
