//! Unique-key owner waits inside REPEATABLE READ transactions report deadlock cycles.

use cassie::app::{CassieError, TransactionErrorKind};
use cassie::types::Value;
use std::sync::{mpsc, Arc, Barrier};
use std::time::{Duration, Instant};

use crate::support_sql_fixture::sql_fixture;

fn wait_for_running_query(cassie: &cassie::app::Cassie) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
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

#[test]
fn should_report_deadlock_for_a_repeatable_read_unique_wait_cycle() {
    // Arrange
    let fixture = sql_fixture(
        "rr-unique-wait-deadlock",
        &["CREATE TABLE rr_wait_deadlock (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let first = fixture.session.clone();
    let victim = fixture.cassie.create_session("second", None);
    for (session, insert) in [
        (&first, "INSERT INTO rr_wait_deadlock VALUES (1, 'first')"),
        (&victim, "INSERT INTO rr_wait_deadlock VALUES (2, 'second')"),
    ] {
        for sql in [
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
            "SELECT count(*) FROM rr_wait_deadlock",
            insert,
        ] {
            fixture
                .cassie
                .execute_sql(session, sql, vec![])
                .expect("stage an owned unique key under a retained view");
        }
    }
    let cassie = fixture.cassie.clone();
    let waiting_session = first.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &waiting_session,
            "INSERT INTO rr_wait_deadlock VALUES (3, 'second')",
            vec![],
        );
        sender.send(result).expect("report the waiting insert");
    });
    started.wait();
    let waiting = wait_for_running_query(&fixture.cassie);

    // Act
    let cycle = fixture.cassie.execute_sql(
        &victim,
        "INSERT INTO rr_wait_deadlock VALUES (4, 'first')",
        vec![],
    );
    let recovered = fixture.cassie.execute_sql(&victim, "ROLLBACK", vec![]);
    let released = receiver.recv_timeout(Duration::from_secs(5));
    waiter.join().expect("waiting insert worker");
    let finished = fixture.cassie.execute_sql(&first, "COMMIT", vec![]);
    let visible = fixture.rows("SELECT id FROM rr_wait_deadlock ORDER BY id");

    // Assert
    assert!(waiting, "the first insert must enter its unique-key wait");
    assert!(
        matches!(
            cycle,
            Err(CassieError::Transaction {
                kind: TransactionErrorKind::DeadlockDetected,
                ..
            })
        ),
        "the second conflicting insert must detect the wait cycle"
    );
    assert!(recovered.is_ok(), "the victim rolls back cleanly");
    assert!(
        released.is_ok_and(|result| result.is_ok()),
        "the waiter proceeds after the victim releases its key"
    );
    assert!(finished.is_ok());
    assert_eq!(visible, vec![vec![Value::Int64(1)], vec![Value::Int64(3)]]);
}
