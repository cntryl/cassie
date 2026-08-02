use cassie::app::{Cassie, CassieSession};
use cassie::runtime::QueryCancellationHandle;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, use_local_storage};

fn with_copy_table(test_name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    use_local_storage();
    let path = data_dir(test_name);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_boundary_rows (id INT PRIMARY KEY, title TEXT)",
            vec![],
        )
        .expect("create table");
    test(&cassie, &session);
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

fn copy_statement() -> CopyStatement {
    CopyStatement {
        table: "copy_boundary_rows".to_string(),
        columns: vec!["id".to_string(), "title".to_string()],
        format: CopyFormat::Csv,
        header: false,
    }
}

fn selected_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT title FROM copy_boundary_rows ORDER BY title",
            vec![],
        )
        .expect("select rows")
        .rows
}

#[test]
fn should_rollback_copy_from_stdin_given_a_malformed_csv_row() {
    // Arrange
    with_copy_table("copy-malformed-row", |cassie, session| {
        let statement = copy_statement();

        // Act
        let result =
            cassie.copy_from_csv_stdin(session, &statement, b"1,alpha\n2,\"unterminated\n");

        // Assert
        assert!(result.is_err());
        assert!(selected_rows(cassie, session).is_empty());
    });
}

#[test]
fn should_preserve_prior_rows_given_a_later_copy_row_failure() {
    // Arrange
    with_copy_table("copy-later-row-failure", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "INSERT INTO copy_boundary_rows (id, title) VALUES (1, 'existing')",
                vec![],
            )
            .expect("seed existing row");
        let statement = copy_statement();

        // Act
        let result = cassie.copy_from_csv_stdin(
            session,
            &statement,
            b"2,staged-before-error\nnot-an-integer,invalid\n",
        );

        // Assert
        assert!(result.is_err());
        assert_eq!(
            selected_rows(cassie, session),
            vec![vec![Value::String("existing".into())]]
        );
    });
}

#[test]
fn should_reject_copy_inside_a_failed_transaction() {
    // Arrange
    with_copy_table("copy-failed-transaction", |cassie, session| {
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(session, "SELECT 1 / 0", vec![])
            .expect_err("fail transaction");

        // Act
        let result = cassie.copy_from_csv_stdin(session, &copy_statement(), b"1,blocked\n");

        // Assert
        assert!(result.is_err());
        assert_eq!(session.transaction_status(), "failed");
        cassie
            .execute_sql(session, "ROLLBACK", vec![])
            .expect("rollback");
    });
}

#[test]
fn should_require_rollback_after_copy_failure() {
    // Arrange
    with_copy_table("copy-requires-rollback", |cassie, session| {
        cassie
            .execute_sql(session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .copy_from_csv_stdin(session, &copy_statement(), b"invalid,blocked\n")
            .expect_err("copy failure");

        // Act
        let blocked = cassie.execute_sql(session, "SELECT 1", vec![]);
        let rollback = cassie.execute_sql(session, "ROLLBACK", vec![]);
        let recovered = cassie.execute_sql(session, "SELECT 1", vec![]);

        // Assert
        assert!(blocked.is_err());
        assert!(rollback.is_ok());
        assert!(recovered.is_ok());
    });
}

#[test]
fn should_cancel_copy_before_partial_commit() {
    // Arrange
    with_copy_table("copy-cancel-before-commit", |cassie, session| {
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();

        // Act
        let result = cassie.copy_from_csv_stdin_with_cancellation(
            session,
            &copy_statement(),
            b"1,first\n2,second\n",
            &cancellation,
        );

        // Assert
        assert!(result.is_err());
        assert!(selected_rows(cassie, session).is_empty());
    });
}
