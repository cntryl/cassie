//! PostgreSQL 18.6 transaction snapshots tracked by #819.

use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_keep_repeatable_read_snapshot_after_concurrent_commit() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-repeatable-read-snapshot",
        &[
            "CREATE TABLE issue_819_snapshot (id INT PRIMARY KEY, value INT)",
            "INSERT INTO issue_819_snapshot VALUES (1, 10)",
        ],
    );
    let reader = fixture.session.clone();
    let writer = fixture.cassie.create_session("writer", None);

    // Act
    fixture
        .cassie
        .execute_sql(
            &reader,
            "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY",
            vec![],
        )
        .expect("begin a read-only repeatable-read transaction");
    let before_commit = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT value FROM issue_819_snapshot WHERE id = 1",
            vec![],
        )
        .expect("read the initial snapshot");
    fixture
        .cassie
        .execute_sql(
            &writer,
            "UPDATE issue_819_snapshot SET value = 20 WHERE id = 1",
            vec![],
        )
        .expect("commit an update from another session");
    let after_commit = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT value FROM issue_819_snapshot WHERE id = 1",
            vec![],
        )
        .expect("read again inside the same snapshot");
    fixture
        .cassie
        .execute_sql(&reader, "COMMIT", vec![])
        .expect("finish the reader transaction");

    // Assert
    assert_eq!(before_commit.rows(), &[vec![Value::Int64(10)]]);
    assert_eq!(after_commit.rows(), &[vec![Value::Int64(10)]]);
}

#[test]
fn should_apply_repeatable_read_set_transaction_before_snapshot() {
    // Arrange
    let fixture = sql_fixture(
        "issue-819-set-transaction-repeatable-read",
        &[
            "CREATE TABLE issue_819_set_transaction_snapshot \
               (id INT PRIMARY KEY, value INT)",
            "INSERT INTO issue_819_set_transaction_snapshot VALUES (1, 10)",
        ],
    );
    let reader = fixture.session.clone();
    let writer = fixture.cassie.create_session("writer", None);
    fixture
        .cassie
        .execute_sql(&reader, "BEGIN", vec![])
        .expect("begin with the default READ COMMITTED mode");

    // Act
    fixture
        .cassie
        .execute_sql(
            &reader,
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            vec![],
        )
        .expect("select REPEATABLE READ before the first snapshot");
    let before_update = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT value FROM issue_819_set_transaction_snapshot WHERE id = 1",
            vec![],
        )
        .expect("establish the repeatable-read snapshot")
        .rows()
        .to_vec();
    fixture
        .cassie
        .execute_sql(
            &writer,
            "UPDATE issue_819_set_transaction_snapshot SET value = 20 WHERE id = 1",
            vec![],
        )
        .expect("commit a concurrent update from the writer session");
    let inside_snapshot_after_update = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT value FROM issue_819_set_transaction_snapshot WHERE id = 1",
            vec![],
        )
        .expect("read again from the same repeatable-read snapshot")
        .rows()
        .to_vec();
    fixture
        .cassie
        .execute_sql(&reader, "COMMIT", vec![])
        .expect("finish the repeatable-read transaction");
    let after_reader_commit = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT value FROM issue_819_set_transaction_snapshot WHERE id = 1",
            vec![],
        )
        .expect("read using the next transaction snapshot")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(before_update, vec![vec![Value::Int64(10)]]);
    assert_eq!(
        inside_snapshot_after_update,
        vec![vec![Value::Int64(10)]],
        "the repeatable-read snapshot must not absorb the writer's later commit"
    );
    assert_eq!(after_reader_commit, vec![vec![Value::Int64(20)]]);
}
