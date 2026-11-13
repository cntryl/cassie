//! One explicit transaction over a row table and a column table commits,
//! rolls back and fails as a unit (issue #1108).

use cassie::app::{Cassie, CassieError, TransactionErrorKind};
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    DocumentWriteFailurePoint,
};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const SETUP: [&str; 4] = [
    "CREATE TABLE mixed_row (id INT PRIMARY KEY, v TEXT)",
    "CREATE TABLE mixed_col (id INT PRIMARY KEY, v TEXT) WITH (storage = column_store)",
    "INSERT INTO mixed_row VALUES (1, 'row-seed')",
    "INSERT INTO mixed_col VALUES (1, 'col-seed')",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, &SETUP)
}

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

fn both(fixture: &SqlFixture) -> (Vec<Vec<Value>>, Vec<Vec<Value>>) {
    (
        fixture.rows("SELECT id, v FROM mixed_row ORDER BY id"),
        fixture.rows("SELECT id, v FROM mixed_col ORDER BY id"),
    )
}

fn seeded_state() -> (Vec<Vec<Value>>, Vec<Vec<Value>>) {
    (
        vec![vec![Value::Int64(1), text("row-seed")]],
        vec![vec![Value::Int64(1), text("col-seed")]],
    )
}

fn stage_both(fixture: &SqlFixture) {
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("INSERT INTO mixed_row VALUES (2, 'row-new')")
        .expect("stage row table");
    fixture
        .execute("INSERT INTO mixed_col VALUES (2, 'col-new')")
        .expect("stage column table");
}

#[test]
fn should_read_own_writes_in_both_modes_and_hide_them_from_other_sessions() {
    // Arrange
    let fixture = fixture("mixed-read-own-writes");
    let other = fixture.cassie.create_session("tester", None);

    // Act
    stage_both(&fixture);
    let (own_row, own_col) = both(&fixture);
    let outside = fixture
        .cassie
        .execute_sql(&other, "SELECT count(*) FROM mixed_col", vec![])
        .expect("other session read");

    // Assert
    assert_eq!(own_row.len(), 2);
    assert_eq!(own_col.len(), 2);
    assert_eq!(outside.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_commit_both_modes_together() {
    // Arrange
    let fixture = fixture("mixed-commit");
    stage_both(&fixture);

    // Act
    fixture.execute("COMMIT").expect("commit");
    let (row, col) = both(&fixture);

    // Assert
    assert_eq!(row.len(), 2);
    assert_eq!(col.len(), 2);
    assert_eq!(col[1], vec![Value::Int64(2), text("col-new")]);
}

#[test]
fn should_roll_back_both_modes_together() {
    // Arrange
    let fixture = fixture("mixed-rollback");
    stage_both(&fixture);

    // Act
    fixture.execute("ROLLBACK").expect("rollback");

    // Assert
    assert_eq!(both(&fixture), seeded_state());
}

#[test]
fn should_roll_back_both_modes_to_a_savepoint() {
    // Arrange
    let fixture = fixture("mixed-savepoint");
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("INSERT INTO mixed_row VALUES (2, 'kept-row')")
        .expect("stage kept row");
    fixture
        .execute("INSERT INTO mixed_col VALUES (2, 'kept-col')")
        .expect("stage kept column row");
    fixture.execute("SAVEPOINT sp").expect("savepoint");
    fixture
        .execute("INSERT INTO mixed_row VALUES (3, 'dropped-row')")
        .expect("stage dropped row");
    fixture
        .execute("UPDATE mixed_col SET v = 'dropped-update' WHERE id = 1")
        .expect("stage dropped update");
    fixture
        .execute("DELETE FROM mixed_col WHERE id = 2")
        .expect("stage dropped delete");

    // Act
    fixture
        .execute("ROLLBACK TO SAVEPOINT sp")
        .expect("rollback to");
    let inside = both(&fixture);
    fixture.execute("COMMIT").expect("commit");
    let committed = both(&fixture);

    // Assert
    assert_eq!(inside, committed);
    assert_eq!(committed.0.len(), 2);
    assert_eq!(
        committed.1,
        vec![
            vec![Value::Int64(1), text("col-seed")],
            vec![Value::Int64(2), text("kept-col")]
        ]
    );
}

#[test]
fn should_fail_the_transaction_when_a_column_table_write_fails() {
    // Arrange
    let fixture = fixture("mixed-failed-state");
    fixture.execute("BEGIN").expect("begin");
    fixture
        .execute("INSERT INTO mixed_row VALUES (2, 'row-new')")
        .expect("stage row table");

    // Act
    let duplicate = fixture.execute("INSERT INTO mixed_col VALUES (1, 'duplicate key')");
    let blocked = fixture.execute("SELECT id FROM mixed_col");
    fixture
        .execute("ROLLBACK")
        .expect("rollback failed transaction");

    // Assert
    assert!(duplicate.is_err());
    assert!(matches!(
        blocked,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::FailedTransaction,
            ..
        })
    ));
    assert_eq!(both(&fixture), seeded_state());
}

#[test]
fn should_leave_neither_table_changed_after_an_injected_commit_failure() {
    // Arrange
    let _failpoint_guard = document_write_failure_point_test_guard();
    let fixture = fixture("mixed-commit-failure");
    stage_both(&fixture);
    set_document_write_failure_point(Some(DocumentWriteFailurePoint::Row));

    // Act
    let commit = fixture.execute("COMMIT");
    set_document_write_failure_point(None);
    let _ = fixture.execute("ROLLBACK");

    // Assert
    assert!(commit.is_err());
    assert_eq!(both(&fixture), seeded_state());
}

#[test]
fn should_commit_copy_into_a_column_table_inside_a_transaction() {
    // Arrange
    let fixture = fixture("mixed-copy");
    let statement = CopyStatement {
        table: "mixed_col".to_string(),
        columns: vec!["id".to_string(), "v".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };
    fixture.execute("BEGIN").expect("begin");

    // Act
    let copied = fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, b"5,five\n6,six\n")
        .expect("copy in transaction");
    let own = fixture.rows("SELECT count(*) FROM mixed_col");
    fixture.execute("ROLLBACK").expect("rollback");
    let after_rollback = fixture.rows("SELECT count(*) FROM mixed_col");
    fixture.execute("BEGIN").expect("begin again");
    fixture
        .cassie
        .copy_from_csv_stdin(&fixture.session, &statement, b"5,five\n6,six\n")
        .expect("copy again");
    fixture.execute("COMMIT").expect("commit");
    let committed = fixture.rows("SELECT count(*) FROM mixed_col");

    // Assert
    assert_eq!(copied, 2);
    assert_eq!(own, vec![vec![Value::Int64(3)]]);
    assert_eq!(after_rollback, vec![vec![Value::Int64(1)]]);
    assert_eq!(committed, vec![vec![Value::Int64(3)]]);
}

#[test]
fn should_reject_a_repeatable_read_update_of_a_column_row_changed_by_another_session() {
    // Arrange
    let fixture = fixture("mixed-repeatable-read");
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    let before = fixture.rows("SELECT v FROM mixed_col WHERE id = 1");
    let writer = fixture.cassie.create_session("tester", None);
    fixture
        .cassie
        .execute_sql(
            &writer,
            "UPDATE mixed_col SET v = 'concurrent' WHERE id = 1",
            vec![],
        )
        .expect("competing commit");

    // Act
    let outcome = fixture
        .execute("UPDATE mixed_col SET v = 'mine' WHERE id = 1")
        .and_then(|_| fixture.execute("COMMIT"));
    let _ = fixture.execute("ROLLBACK");

    // Assert
    assert_eq!(before, vec![vec![text("col-seed")]]);
    assert!(matches!(
        outcome,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::SerializationFailure,
            ..
        })
    ));
    assert_eq!(
        fixture.rows("SELECT v FROM mixed_col WHERE id = 1"),
        vec![vec![text("concurrent")]]
    );
}

#[test]
fn should_reject_a_repeatable_read_update_after_another_session_deleted_and_reinserted_a_column_row(
) {
    // Arrange
    let fixture = fixture("mixed-repeatable-read-reinsert");
    fixture
        .execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE")
        .expect("begin");
    let _ = fixture.rows("SELECT v FROM mixed_col WHERE id = 1");
    let writer = fixture.cassie.create_session("tester", None);
    for sql in [
        "DELETE FROM mixed_col WHERE id = 1",
        "INSERT INTO mixed_col VALUES (1, 'col-seed')",
    ] {
        fixture
            .cassie
            .execute_sql(&writer, sql, vec![])
            .expect("competing commit");
    }

    // Act
    let outcome = fixture
        .execute("UPDATE mixed_col SET v = 'mine' WHERE id = 1")
        .and_then(|_| fixture.execute("COMMIT"));
    let _ = fixture.execute("ROLLBACK");

    // Assert
    assert!(matches!(
        outcome,
        Err(CassieError::Transaction {
            kind: TransactionErrorKind::SerializationFailure,
            ..
        })
    ));
}

#[test]
fn should_show_both_modes_after_restart_when_one_transaction_committed_them() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("mixed-mode-restart");
    let _directory = DataDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("first instance");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in SETUP.iter().copied().chain([
            "BEGIN",
            "INSERT INTO mixed_row VALUES (2, 'row-new')",
            "INSERT INTO mixed_col VALUES (2, 'col-new')",
            "COMMIT",
        ]) {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("statement failed: {sql}: {error}"));
        }
    }

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopened instance");
    reopened.startup().expect("recovery");
    let session = reopened.create_session("tester", None);
    let counts = ["mixed_row", "mixed_col"].map(|table| {
        reopened
            .execute_sql(&session, &format!("SELECT count(*) FROM {table}"), vec![])
            .unwrap_or_else(|error| panic!("count failed: {table}: {error}"))
            .rows()
            .to_vec()
    });
    let violations = reopened
        .midge
        .verify_column_store_for_diagnostics("mixed_col")
        .expect("verifier");

    // Assert
    assert_eq!(
        counts,
        [vec![vec![Value::Int64(2)]], vec![vec![Value::Int64(2)]]]
    );
    assert!(violations.is_empty(), "{violations:?}");
}

struct DataDirectory(String);

impl Drop for DataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
