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
