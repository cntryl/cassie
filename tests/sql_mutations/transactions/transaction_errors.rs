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

include!("transaction_errors/transaction_state.rs");
include!("transaction_errors/unique_conflicts.rs");
include!("transaction_errors/deadlock_recovery.rs");
include!("transaction_errors/transactional_ddl.rs");
