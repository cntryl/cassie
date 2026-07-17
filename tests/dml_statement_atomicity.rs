use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::runtime::QueryCancellationHandle;
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn data_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-dml-atomicity-{label}-{}", Uuid::new_v4()))
}

fn database(label: &str) -> (Cassie, CassieSession, PathBuf) {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session, path)
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("select rows")
        .rows
}

#[test]
fn should_leave_zero_rows_given_later_unique_failure_when_inserting_multiple_rows() {
    // Arrange
    let (cassie, session, path) = database("insert-unique");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_unique_insert (row_key INT PRIMARY KEY, email TEXT UNIQUE)",
            vec![],
        )
        .expect("create table");

    // Act
    let result = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_unique_insert (row_key, email) VALUES (1, 'same@example.com'), (2, 'same@example.com')",
        vec![],
    );

    // Assert
    assert!(
        result.is_err(),
        "later duplicate should reject the statement"
    );
    assert!(
        rows(
            &cassie,
            &session,
            "SELECT row_key, email FROM atomic_unique_insert ORDER BY row_key"
        )
        .is_empty(),
        "a failed statement must not retain its earlier source rows"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_leave_zero_rows_given_later_check_failure_when_inserting_multiple_rows() {
    // Arrange
    let (cassie, session, path) = database("insert-check");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_check_insert (row_key INT PRIMARY KEY, score INT CHECK (score >= 0))",
            vec![],
        )
        .expect("create table");

    // Act
    let result = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_check_insert (row_key, score) VALUES (1, 10), (2, -1)",
        vec![],
    );

    // Assert
    assert!(
        result.is_err(),
        "later check failure should reject the statement"
    );
    assert!(
        rows(
            &cassie,
            &session,
            "SELECT row_key, score FROM atomic_check_insert ORDER BY row_key"
        )
        .is_empty(),
        "a failed statement must not retain its earlier source rows"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_leave_zero_child_rows_given_later_foreign_key_failure_when_inserting_multiple_rows() {
    // Arrange
    let (cassie, session, path) = database("insert-foreign-key");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_fk_parents (row_key INT PRIMARY KEY)",
            vec![],
        )
        .expect("create parent table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_fk_children (row_key INT PRIMARY KEY, parent_key INT REFERENCES atomic_fk_parents(row_key))",
            vec![],
        )
        .expect("create child table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_fk_parents (row_key) VALUES (1)",
            vec![],
        )
        .expect("seed parent");

    // Act
    let result = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_fk_children (row_key, parent_key) VALUES (1, 1), (2, 999)",
        vec![],
    );

    // Assert
    assert!(
        result.is_err(),
        "later foreign key failure should reject the statement"
    );
    assert!(
        rows(
            &cassie,
            &session,
            "SELECT row_key, parent_key FROM atomic_fk_children ORDER BY row_key"
        )
        .is_empty(),
        "a failed statement must not retain its earlier source rows"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_leave_all_rows_unchanged_given_multi_row_update_unique_conflict() {
    // Arrange
    let (cassie, session, path) = database("update-unique");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_unique_update (row_key INT PRIMARY KEY, email TEXT UNIQUE)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_unique_update (row_key, email) VALUES (1, 'one@example.com'), (2, 'two@example.com')",
            vec![],
        )
        .expect("seed rows");

    // Act
    let result = cassie.execute_sql(
        &session,
        "UPDATE atomic_unique_update SET email = 'same@example.com'",
        vec![],
    );

    // Assert
    assert!(result.is_err(), "unique conflict should reject the update");
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key, email FROM atomic_unique_update ORDER BY row_key"
        ),
        vec![
            vec![
                Value::Int64(1),
                Value::String("one@example.com".to_string())
            ],
            vec![
                Value::Int64(2),
                Value::String("two@example.com".to_string())
            ],
        ]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_restore_all_collections_given_delete_cascade_followed_by_restrict_failure() {
    // Arrange
    let (cassie, session, path) = database("delete-cascade-failure");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_delete_parents (row_key INT PRIMARY KEY)",
            vec![],
        )
        .expect("create parent table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_delete_a_cascade (row_key INT PRIMARY KEY, parent_key INT, CONSTRAINT atomic_delete_a_cascade_fk FOREIGN KEY (parent_key) REFERENCES atomic_delete_parents(row_key) ON DELETE CASCADE)",
            vec![],
        )
        .expect("create cascade child table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_delete_z_restrict (row_key INT PRIMARY KEY, parent_key INT REFERENCES atomic_delete_parents(row_key))",
            vec![],
        )
        .expect("create restricting child table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_delete_parents (row_key) VALUES (1)",
            vec![],
        )
        .expect("seed parent");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_delete_a_cascade (row_key, parent_key) VALUES (10, 1)",
            vec![],
        )
        .expect("seed cascade child");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_delete_z_restrict (row_key, parent_key) VALUES (20, 1)",
            vec![],
        )
        .expect("seed restricting child");

    // Act
    let result = cassie.execute_sql(
        &session,
        "DELETE FROM atomic_delete_parents WHERE row_key = 1",
        vec![],
    );

    // Assert
    assert!(
        result.is_err(),
        "restricting child should reject the delete"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key FROM atomic_delete_parents"
        ),
        vec![vec![Value::Int64(1)]]
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key FROM atomic_delete_a_cascade"
        ),
        vec![vec![Value::Int64(10)]],
        "the earlier cascade must be rolled back"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key FROM atomic_delete_z_restrict"
        ),
        vec![vec![Value::Int64(20)]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_restore_all_collections_given_update_cascade_constraint_failure() {
    // Arrange
    let (cassie, session, path) = database("update-cascade-failure");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_update_parents (row_key INT PRIMARY KEY)",
            vec![],
        )
        .expect("create parent table");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_update_children (row_key INT PRIMARY KEY, parent_key INT CHECK (parent_key < 2), CONSTRAINT atomic_update_children_fk FOREIGN KEY (parent_key) REFERENCES atomic_update_parents(row_key) ON UPDATE CASCADE)",
            vec![],
        )
        .expect("create child table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_update_parents (row_key) VALUES (1)",
            vec![],
        )
        .expect("seed parent");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_update_children (row_key, parent_key) VALUES (10, 1)",
            vec![],
        )
        .expect("seed child");

    // Act
    let result = cassie.execute_sql(
        &session,
        "UPDATE atomic_update_parents SET row_key = 2 WHERE row_key = 1",
        vec![],
    );

    // Assert
    assert!(
        result.is_err(),
        "cascaded child check failure should reject the update"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key FROM atomic_update_parents"
        ),
        vec![vec![Value::Int64(1)]],
        "the parent update must be rolled back"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key, parent_key FROM atomic_update_children"
        ),
        vec![vec![Value::Int64(10), Value::Int64(1)]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_prior_transaction_work_given_failed_statement_rolled_back_to_savepoint() {
    // Arrange
    let (cassie, session, path) = database("transaction-savepoint");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_transaction_rows (row_key INT PRIMARY KEY, score INT CHECK (score >= 0))",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_transaction_rows (row_key, score) VALUES (1, 10)",
            vec![],
        )
        .expect("stage earlier statement");
    cassie
        .execute_sql(&session, "SAVEPOINT before_failure", vec![])
        .expect("create savepoint");

    // Act
    let failed = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_transaction_rows (row_key, score) VALUES (2, 20), (3, -1) RETURNING row_key",
        vec![],
    );
    cassie
        .execute_sql(&session, "ROLLBACK TO SAVEPOINT before_failure", vec![])
        .expect("recover failed transaction");
    let visible_after_recovery = rows(
        &cassie,
        &session,
        "SELECT row_key, score FROM atomic_transaction_rows ORDER BY row_key",
    );
    cassie
        .execute_sql(&session, "COMMIT", vec![])
        .expect("commit earlier work");

    // Assert
    assert!(
        failed.is_err(),
        "later check failure should reject the statement"
    );
    assert_eq!(
        visible_after_recovery,
        vec![vec![Value::Int64(1), Value::Int64(10)]],
        "savepoint recovery must preserve prior work without partial failed-statement writes"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key, score FROM atomic_transaction_rows ORDER BY row_key"
        ),
        visible_after_recovery
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rollback_prior_source_rows_given_later_on_conflict_update_failure() {
    // Arrange
    let (cassie, session, path) = database("upsert-atomicity");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_upsert_rows (row_key INT PRIMARY KEY, email TEXT UNIQUE, note TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_upsert_rows (row_key, email, note) VALUES (1, 'one@example.com', 'original')",
            vec![],
        )
        .expect("seed conflict row");

    // Act
    let result = cassie.execute_sql(
        &session,
        "INSERT INTO atomic_upsert_rows (row_key, email, note) VALUES (2, 'two@example.com', 'inserted'), (1, 'two@example.com', 'updated') ON CONFLICT (row_key) DO UPDATE SET email = excluded.email, note = excluded.note RETURNING row_key, email, note",
        vec![],
    );

    // Assert
    assert!(
        result.is_err(),
        "unique failure in conflict update should reject the statement"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            "SELECT row_key, email, note FROM atomic_upsert_rows ORDER BY row_key"
        ),
        vec![vec![
            Value::Int64(1),
            Value::String("one@example.com".to_string()),
            Value::String("original".to_string())
        ]],
        "the earlier source-row insert must be rolled back"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_source_order_given_multi_row_on_conflict_returning() {
    // Arrange
    let (cassie, session, path) = database("upsert-returning-order");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_upsert_order (row_key INT PRIMARY KEY, title TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_upsert_order (row_key, title) VALUES (2, 'old')",
            vec![],
        )
        .expect("seed conflict row");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "INSERT INTO atomic_upsert_order (row_key, title) VALUES (3, 'third'), (2, 'updated'), (1, 'first') ON CONFLICT (row_key) DO UPDATE SET title = excluded.title RETURNING row_key, title",
            vec![],
        )
        .expect("execute upsert");

    // Assert
    assert_eq!(result.command, "INSERT 0 3");
    assert_eq!(
        result.rows,
        vec![
            vec![Value::Int64(3), Value::String("third".to_string())],
            vec![Value::Int64(2), Value::String("updated".to_string())],
            vec![Value::Int64(1), Value::String("first".to_string())],
        ]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_leave_zero_rows_given_cancelled_multi_row_insert_before_execution() {
    // Arrange
    let (cassie, session, path) = database("cancel-before-execution");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE atomic_cancelled_insert (row_key INT PRIMARY KEY)",
            vec![],
        )
        .expect("create table");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();

    // Act
    let error = cassie
        .execute_sql_with_cancellation(
            &session,
            "INSERT INTO atomic_cancelled_insert (row_key) VALUES (1), (2), (3) RETURNING row_key",
            vec![],
            &cancellation,
        )
        .expect_err("cancelled mutation should fail");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert!(
        rows(
            &cassie,
            &session,
            "SELECT row_key FROM atomic_cancelled_insert"
        )
        .is_empty(),
        "a cancelled mutation must not write rows"
    );

    let _ = std::fs::remove_dir_all(path);
}
