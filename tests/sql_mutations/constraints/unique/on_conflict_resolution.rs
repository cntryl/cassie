use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use super::support_sql as support;
use support::{data_dir, use_local_storage};

fn start(label: &str) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session, path)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    assert!(
        cassie.execute_sql(session, sql, vec![]).is_ok(),
        "statement should succeed: {sql}"
    );
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .map(|result| result.rows().to_vec())
        .unwrap_or_default()
}

fn seed_expression_index(cassie: &Cassie, session: &CassieSession) {
    run(
        cassie,
        session,
        "CREATE TABLE a1 (id INT PRIMARY KEY, email TEXT)",
    );
    run(
        cassie,
        session,
        "CREATE UNIQUE INDEX a1_lower ON a1 ((lower(email)))",
    );
    run(
        cassie,
        session,
        "INSERT INTO a1 (id, email) VALUES (1, 'A@B.com')",
    );
}

#[test]
fn should_skip_expression_index_conflict_given_untargeted_do_nothing() {
    // Arrange
    let (cassie, session, path) = start("on-conflict-expression-autocommit");
    seed_expression_index(&cassie, &session);

    // Act
    let inserted = cassie.execute_sql(
        &session,
        "INSERT INTO a1 (id, email) VALUES (2, 'a@b.com') ON CONFLICT DO NOTHING",
        vec![],
    );

    // Assert
    assert!(
        inserted.is_ok(),
        "ON CONFLICT DO NOTHING skips the duplicate"
    );
    assert_eq!(
        rows(&cassie, &session, "SELECT id FROM a1"),
        vec![vec![Value::Int64(1)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_skip_expression_index_conflict_given_untargeted_do_nothing_in_transaction() {
    // Arrange
    let (cassie, session, path) = start("on-conflict-expression-transaction");
    seed_expression_index(&cassie, &session);
    run(&cassie, &session, "BEGIN");

    // Act
    run(
        &cassie,
        &session,
        "INSERT INTO a1 (id, email) VALUES (2, 'a@b.com') ON CONFLICT DO NOTHING",
    );
    let committed = cassie.execute_sql(&session, "COMMIT", vec![]);

    // Assert
    assert!(
        committed.is_ok(),
        "COMMIT succeeds with the duplicate skipped"
    );
    assert_eq!(
        rows(&cassie, &session, "SELECT id FROM a1"),
        vec![vec![Value::Int64(1)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_fold_unquoted_conflict_target_to_catalog_column() {
    // Arrange
    let (cassie, session, path) = start("on-conflict-target-case");
    run(
        &cassie,
        &session,
        "CREATE TABLE f5 (id INT PRIMARY KEY, email TEXT UNIQUE)",
    );
    run(
        &cassie,
        &session,
        "INSERT INTO f5 (id, email) VALUES (1, 'x')",
    );

    // Act
    let skipped = cassie.execute_sql(
        &session,
        "INSERT INTO f5 (id, email) VALUES (2, 'x') ON CONFLICT (EMAIL) DO NOTHING",
        vec![],
    );
    let updated = cassie.execute_sql(
        &session,
        "INSERT INTO f5 (id, email) VALUES (3, 'x') ON CONFLICT (Email) DO UPDATE SET id = 9",
        vec![],
    );

    // Assert
    assert!(skipped.is_ok(), "ON CONFLICT (EMAIL) DO NOTHING");
    assert!(updated.is_ok(), "ON CONFLICT (Email) DO UPDATE");
    assert_eq!(
        rows(&cassie, &session, "SELECT id FROM f5"),
        vec![vec![Value::Int64(9)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_do_update_that_affects_a_row_twice() {
    // Arrange
    let (cassie, session, path) = start("on-conflict-row-twice");
    run(&cassie, &session, "CREATE TABLE t (k TEXT UNIQUE, v INT)");
    run(&cassie, &session, "INSERT INTO t (k, v) VALUES ('a', 1)");

    // Act
    let upserted = cassie.execute_sql(
            &session,
            "INSERT INTO t (k, v) VALUES ('a', 2), ('a', 3) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            vec![],
        );

    // Assert
    assert!(
        matches!(
            upserted,
            Err(cassie::app::CassieError::CardinalityViolation(_))
        ),
        "a row may not be affected twice by one ON CONFLICT DO UPDATE"
    );
    assert_eq!(
        rows(&cassie, &session, "SELECT k, v FROM t"),
        vec![vec![Value::String("a".into()), Value::Int64(1)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_do_update_that_updates_a_row_inserted_by_the_same_statement() {
    // Arrange
    let (cassie, session, path) = start("on-conflict-row-inserted-then-updated");
    run(&cassie, &session, "CREATE TABLE t (k TEXT UNIQUE, v INT)");

    // Act
    let upserted = cassie.execute_sql(
            &session,
            "INSERT INTO t (k, v) VALUES ('b', 1), ('b', 2) ON CONFLICT (k) DO UPDATE SET v = excluded.v",
            vec![],
        );

    // Assert
    assert!(
        matches!(
            upserted,
            Err(cassie::app::CassieError::CardinalityViolation(_))
        ),
        "a row inserted by the statement may not be updated by it"
    );
    assert_eq!(
        rows(&cassie, &session, "SELECT k FROM t"),
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    let _ = std::fs::remove_dir_all(path);
}
