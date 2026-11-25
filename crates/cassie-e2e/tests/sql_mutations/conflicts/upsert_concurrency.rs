use std::sync::{Arc, Barrier};

use super::support_sql as support;

#[test]
fn should_resolve_racing_do_nothing_without_unique_error() {
    // Arrange
    let path = support::data_dir("upsert-racing-do-nothing");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE racing_upserts (email TEXT UNIQUE, value INT)",
            vec![],
        )
        .expect("create table");
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for value in [1, 2] {
        let cassie = cassie.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let session = cassie.create_session("tester", None);
            barrier.wait();
            cassie.execute_sql(
                &session,
                &format!(
                    "INSERT INTO racing_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO NOTHING"
                ),
                vec![],
            )
        }));
    }

    // Act
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();

    // Assert
    assert!(results.iter().all(Result::is_ok));
    let rows = cassie
        .execute_sql(&setup, "SELECT email FROM racing_upserts", vec![])
        .expect("read winner");
    assert_eq!(rows.rows().len(), 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_one_racing_do_update_against_committed_winner() {
    // Arrange
    let path = support::data_dir("upsert-racing-do-update");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE racing_update_upserts (email TEXT UNIQUE, value INT)",
            vec![],
        )
        .expect("create table");
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for value in [1, 2] {
        let cassie = cassie.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let session = cassie.create_session("tester", None);
            barrier.wait();
            cassie.execute_sql(
                &session,
                &format!(
                    "INSERT INTO racing_update_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO UPDATE SET value = excluded.value RETURNING value"
                ),
                vec![],
            )
        }));
    }

    // Act
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().expect("worker"))
        .collect::<Vec<_>>();

    // Assert
    assert!(results.iter().all(Result::is_ok));
    assert!(results
        .iter()
        .all(|result| result.as_ref().expect("upsert").rows().len() == 1));
    let rows = cassie
        .execute_sql(&setup, "SELECT value FROM racing_update_upserts", vec![])
        .expect("read resolved row");
    assert_eq!(rows.rows().len(), 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_transactional_do_nothing_when_competing_commit_wins() {
    // Arrange
    let path = support::data_dir("upsert-transaction-do-nothing");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let setup = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE transaction_upserts (email TEXT UNIQUE, value INT)",
            vec![],
        )
        .expect("create table");
    let first = cassie.create_session("tester", None);
    let second = cassie.create_session("tester", None);
    for session in [&first, &second] {
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");
    }
    cassie.execute_sql(&first,
        "INSERT INTO transaction_upserts (email, value) VALUES ('same@example.com', 1) ON CONFLICT (email) DO NOTHING",
        vec![]).expect("stage owner upsert");

    // Act
    let changed = finish_waiting_upsert(&cassie, &first, &second,
        "INSERT INTO transaction_upserts (email, value) VALUES ('same@example.com', 2) ON CONFLICT (email) DO NOTHING");
    cassie
        .execute_sql(&second, "COMMIT", vec![])
        .expect("commit resolved waiter");

    // Assert
    assert_eq!(changed.command(), "INSERT 0 0");
    let rows = cassie
        .execute_sql(&setup, "SELECT email FROM transaction_upserts", vec![])
        .expect("read winner");
    assert_eq!(rows.rows().len(), 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_transactional_do_update_against_committed_winner() {
    // Arrange
    let path = support::data_dir("upsert-transaction-do-update");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let setup = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE transaction_update_upserts (email TEXT UNIQUE, value INT)",
            vec![],
        )
        .expect("create table");
    let first = cassie.create_session("tester", None);
    let second = cassie.create_session("tester", None);
    for session in [&first, &second] {
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");
    }
    cassie.execute_sql(&first,
        "INSERT INTO transaction_update_upserts (email, value) VALUES ('same@example.com', 1) ON CONFLICT (email) DO UPDATE SET value = excluded.value",
        vec![]).expect("stage owner upsert");

    // Act
    let changed = finish_waiting_upsert(&cassie, &first, &second,
        "INSERT INTO transaction_update_upserts (email, value) VALUES ('same@example.com', 2) ON CONFLICT (email) DO UPDATE SET value = excluded.value");
    cassie
        .execute_sql(&second, "COMMIT", vec![])
        .expect("commit resolved waiter");

    // Assert
    assert_eq!(changed.command(), "INSERT 0 1");
    let rows = cassie
        .execute_sql(
            &setup,
            "SELECT value FROM transaction_update_upserts",
            vec![],
        )
        .expect("read resolved row");
    assert_eq!(rows.rows(), vec![vec![cassie::types::Value::Int64(2)]]);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_restore_transactional_conflict_intent_with_savepoint_rollback() {
    // Arrange
    let path = support::data_dir("upsert-transaction-savepoint");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE savepoint_upserts (email TEXT UNIQUE, value INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(&session, "SAVEPOINT before_upsert", vec![])
        .expect("create savepoint");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO savepoint_upserts (email, value) VALUES ('rolled-back@example.com', 1) ON CONFLICT (email) DO NOTHING",
            vec![],
        )
        .expect("stage upsert");

    // Act
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT before_upsert", vec![])
        .expect("rollback intent");
    cassie
        .execute_sql(&session, "COMMIT", vec![])
        .expect("commit transaction");

    // Assert
    let rows = cassie
        .execute_sql(&session, "SELECT email FROM savepoint_upserts", vec![])
        .expect("read rows");
    assert_eq!(rows.rows(), [] as [std::vec::Vec<cassie::types::Value>; 0]);

    let _ = std::fs::remove_dir_all(path);
}

fn finish_waiting_upsert(
    cassie: &cassie::app::Cassie,
    owner: &cassie::app::CassieSession,
    waiter: &cassie::app::CassieSession,
    sql: &str,
) -> cassie::executor::QueryResult {
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| cassie.execute_sql(waiter, sql, vec![]));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while cassie.metrics()["transactions"]["unique_waiters"]
            .as_u64()
            .unwrap_or(0)
            == 0
        {
            assert!(
                !worker.is_finished(),
                "upsert must wait for its unresolved owner"
            );
            assert!(
                std::time::Instant::now() < deadline,
                "upsert did not reach the unique-owner wait boundary"
            );
            std::thread::yield_now();
        }
        assert!(
            !worker.is_finished(),
            "waiter must remain pending before owner commit"
        );
        cassie
            .execute_sql(owner, "COMMIT", vec![])
            .expect("commit owner");
        worker
            .join()
            .expect("waiting worker")
            .expect("recheck owner commit")
    })
}

#[test]
fn should_keep_the_highest_committed_entity_revision_in_both_writer_orders() {
    // Arrange
    use cassie::types::Value;
    let row = |revision, payload: &str| {
        vec![
            Value::Int64(1),
            Value::Int64(revision),
            Value::String(payload.to_owned()),
        ]
    };
    let write = |revision, payload: &str| {
        format!(
            "INSERT INTO entities AS target VALUES (1, {revision}, '{payload}') \
            ON CONFLICT (id) DO UPDATE SET rev = EXCLUDED.rev, payload = EXCLUDED.payload \
            WHERE target.rev < EXCLUDED.rev RETURNING id, rev, payload"
        )
    };
    for (first, second) in [((4, "four"), (5, "five")), ((5, "five"), (4, "four"))] {
        let fixture = crate::support_sql_fixture::sql_fixture(
            "entity-revision-writers",
            &[
                "CREATE TABLE entities (id BIGINT PRIMARY KEY, rev BIGINT, payload TEXT)",
                "INSERT INTO entities VALUES (1, 3, 'three')",
            ],
        );
        let cassie = &fixture.cassie;
        let owner = &fixture.session;
        let waiter = cassie.create_session("waiter", None);
        for session in [owner, &waiter] {
            cassie
                .execute_sql(session, "BEGIN ISOLATION LEVEL READ COMMITTED", vec![])
                .expect("begin projector transaction");
        }

        // Act
        let owner_result = fixture
            .execute(&write(first.0, first.1))
            .expect("stage owner revision");
        let waiter_result =
            finish_waiting_upsert(cassie, owner, &waiter, &write(second.0, second.1));
        cassie
            .execute_sql(&waiter, "COMMIT", vec![])
            .expect("commit resolved waiter");
        let readback = "SELECT id, rev, payload FROM entities";
        let committed = fixture.rows(readback);
        fixture
            .execute("BEGIN")
            .expect("begin rolled-back delivery");
        let provisional = fixture
            .execute(&write(6, "provisional"))
            .expect("stage later delivery");
        fixture
            .execute("ROLLBACK")
            .expect("roll back later revision");
        let fresh = cassie.create_session("fresh", None);
        let after_rollback = cassie
            .execute_sql(&fresh, readback, vec![])
            .expect("read after rollback");

        // Assert
        assert_eq!(owner_result.command(), "INSERT 0 1");
        assert_eq!(owner_result.rows(), [row(first.0, first.1)]);
        if second.0 > first.0 {
            assert_eq!(waiter_result.command(), "INSERT 0 1");
            assert_eq!(waiter_result.rows(), [row(second.0, second.1)]);
        } else {
            assert_eq!(waiter_result.command(), "INSERT 0 0");
            assert_eq!(waiter_result.rows(), [] as [Vec<Value>; 0]);
        }
        assert_eq!(committed, [row(5, "five")]);
        assert_eq!(provisional.rows(), [row(6, "provisional")]);
        assert_eq!(after_rollback.rows(), [row(5, "five")]);
    }
}
