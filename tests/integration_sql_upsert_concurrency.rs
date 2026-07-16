use std::sync::{Arc, Barrier};

use cassie::app::Cassie;

#[path = "support/sql.rs"]
mod support;

#[test]
fn should_resolve_racing_do_nothing_without_unique_error() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("upsert-racing-do-nothing");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
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
    assert_eq!(rows.rows.len(), 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_one_racing_do_update_against_committed_winner() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("upsert-racing-do-update");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("cassie"));
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
        .all(|result| result.as_ref().expect("upsert").rows.len() == 1));
    let rows = cassie
        .execute_sql(&setup, "SELECT value FROM racing_update_upserts", vec![])
        .expect("read resolved row");
    assert_eq!(rows.rows.len(), 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_transactional_do_nothing_when_competing_commit_wins() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("upsert-transaction-do-nothing");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
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
    for (session, value) in [(&first, 1), (&second, 2)] {
        cassie
            .execute_sql(
                session,
                &format!(
                    "INSERT INTO transaction_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO NOTHING"
                ),
                vec![],
            )
            .expect("stage upsert");
    }

    // Act
    cassie
        .execute_sql(&first, "COMMIT", vec![])
        .expect("commit winner");
    let losing_commit = cassie.execute_sql(&second, "COMMIT", vec![]);

    // Assert
    losing_commit.expect("resolve losing transaction");
    let rows = cassie
        .execute_sql(&setup, "SELECT email FROM transaction_upserts", vec![])
        .expect("read winner");
    assert_eq!(rows.rows.len(), 1);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_transactional_do_update_against_committed_winner() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("upsert-transaction-do-update");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
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
    for (session, value) in [(&first, 1), (&second, 2)] {
        cassie
            .execute_sql(
                session,
                &format!(
                    "INSERT INTO transaction_update_upserts (email, value) VALUES ('same@example.com', {value}) ON CONFLICT (email) DO UPDATE SET value = excluded.value"
                ),
                vec![],
            )
            .expect("stage upsert");
    }

    // Act
    cassie
        .execute_sql(&first, "COMMIT", vec![])
        .expect("commit winner");
    cassie
        .execute_sql(&second, "COMMIT", vec![])
        .expect("resolve update transaction");

    // Assert
    let rows = cassie
        .execute_sql(
            &setup,
            "SELECT value FROM transaction_update_upserts",
            vec![],
        )
        .expect("read resolved row");
    assert_eq!(rows.rows, vec![vec![cassie::types::Value::Int64(2)]]);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_restore_transactional_conflict_intent_with_savepoint_rollback() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("upsert-transaction-savepoint");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
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
    assert!(rows.rows.is_empty());

    let _ = std::fs::remove_dir_all(path);
}
