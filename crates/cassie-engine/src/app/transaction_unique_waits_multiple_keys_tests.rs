use super::{is_unique_violation_for, receive_event, start_observed_insert, WaitEvent};
use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;
use std::sync::mpsc;

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
        WaitEvent::Completed(Err(error)) if is_unique_violation_for(&error, "t_b_key")
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
        WaitEvent::Completed(Err(error)) if is_unique_violation_for(&error, "t_a_key")
    ));
    assert!(rollback.is_ok());
}

#[test]
fn should_allow_multi_key_insert_after_wait_owner_rolls_back_without_other_conflicts() {
    // Arrange
    let (cassie, first, second) = two_key_fixture("issue_1466_no_other_conflict");
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
    let independent_insert = cassie.execute_sql(&second, "INSERT INTO t VALUES (9, 3)", Vec::new());
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
    assert!(independent_insert.is_ok());
    assert!(owner_rollback.is_ok());
    assert!(reached_wait);
    assert!(matches!(result_event, WaitEvent::Completed(Ok(_))));
    assert!(commit.is_ok());
    assert_eq!(
        committed_rows,
        vec![
            vec![Value::Int64(1), Value::Int64(2)],
            vec![Value::Int64(9), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_keep_the_prepared_identity_when_a_plain_insert_resumes() {
    // Arrange
    let (cassie, owner, waiter) = two_key_fixture("issue_1466_prepared_identity");
    for sql in [
        "CREATE TABLE identity_wait (id SERIAL PRIMARY KEY, payload TEXT UNIQUE)",
        "BEGIN",
        "INSERT INTO identity_wait (payload) VALUES ('held')",
    ] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("stage the first identity and the blocking unique key");
    }
    cassie
        .execute_sql(&waiter, "BEGIN", Vec::new())
        .expect("begin the waiter transaction");
    let (events, worker) = start_observed_insert(
        cassie.clone(),
        waiter.clone(),
        "INSERT INTO identity_wait (payload) VALUES ('held') RETURNING id",
    );
    let first_event = receive_event(&events);
    let reached_wait = matches!(&first_event, WaitEvent::ReachedWait);

    // Act
    let owner_rollback = cassie.execute_sql(&owner, "ROLLBACK", Vec::new());
    let result = if reached_wait {
        receive_event(&events)
    } else {
        first_event
    };
    worker.join().expect("waiting INSERT worker");
    let commit = cassie.execute_sql(&waiter, "COMMIT", Vec::new());
    let following = cassie
        .execute_sql(
            &owner,
            "INSERT INTO identity_wait (payload) VALUES ('next') RETURNING id",
            Vec::new(),
        )
        .expect("insert with the following identity");

    // Assert
    assert!(owner_rollback.is_ok());
    assert!(reached_wait);
    assert!(matches!(
        result,
        WaitEvent::Completed(Ok(inserted)) if inserted.rows() == [vec![Value::Int64(2)]]
    ));
    assert!(commit.is_ok());
    assert_eq!(following.rows(), &[vec![Value::Int64(3)]]);
}
