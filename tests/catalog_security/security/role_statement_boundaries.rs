use cassie::app::{Cassie, CassieError, CassieSession};

fn assert_insufficient_privilege(result: &Result<cassie::executor::QueryResult, CassieError>) {
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

fn with_roles(_test_name: &str, test: impl FnOnce(&Cassie, &CassieSession, &CassieSession)) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("admin");
    cassie
        .execute_sql(
            &admin,
            "CREATE TABLE role_statement_rows (title TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &admin,
            "CREATE ROLE statement_reader LOGIN PASSWORD 'reader-secret'",
            vec![],
        )
        .expect("create reader");
    let reader = cassie
        .authenticate_role("statement_reader", Some("reader-secret"), None)
        .expect("reader");
    test(&cassie, &admin, &reader);
    drop(cassie);
}

#[test]
fn should_allow_read_only_role_to_execute_statements_given_select_show_set_and_transaction_families(
) {
    // Arrange
    with_roles("role-read-only-statements", |cassie, _admin, reader| {
        let statements = [
            "SELECT title FROM role_statement_rows",
            "SHOW search_path",
            "SET search_path TO public",
            "BEGIN",
            "ROLLBACK",
        ];

        // Act
        let results = statements.map(|sql| cassie.execute_sql(reader, sql, vec![]));

        // Assert
        assert!(results.iter().all(Result::is_ok));
    });
}

#[test]
fn should_reject_read_only_role_explain_for_an_insert_statement() {
    // Arrange
    with_roles("role-explain-insert", |cassie, _admin, reader| {
        // Act
        let result = cassie.execute_sql(
            reader,
            "EXPLAIN INSERT INTO role_statement_rows (title) VALUES ('blocked')",
            vec![],
        );

        // Assert
        assert_insufficient_privilege(&result);
    });
}

#[test]
fn should_reject_read_only_role_explain_for_a_nested_mutating_statement() {
    // Arrange
    with_roles("role-nested-explain-insert", |cassie, _admin, reader| {
        // Act
        let result = cassie.execute_sql(
            reader,
            "EXPLAIN EXPLAIN INSERT INTO role_statement_rows (title) VALUES ('blocked')",
            vec![],
        );

        // Assert
        assert_insufficient_privilege(&result);
    });
}

#[test]
fn should_reject_read_only_role_copy_from_stdin() {
    // Arrange
    with_roles("role-copy-from", |cassie, _admin, reader| {
        // Act
        let result = cassie.execute_sql(
            reader,
            "COPY role_statement_rows FROM STDIN WITH (FORMAT csv)",
            vec![],
        );

        // Assert
        assert_insufficient_privilege(&result);
    });
}

#[test]
fn should_reject_read_only_role_copy_to_stdout() {
    // Arrange
    with_roles("role-copy-to", |cassie, _admin, reader| {
        // Act
        let result = cassie.execute_sql(
            reader,
            "COPY role_statement_rows TO STDOUT WITH (FORMAT csv)",
            vec![],
        );

        // Assert
        assert_insufficient_privilege(&result);
    });
}

#[test]
fn should_allow_admin_role_to_execute_mutating_explain_statements() {
    // Arrange
    with_roles("role-admin-explain", |cassie, admin, _reader| {
        // Act
        let result = cassie.execute_sql(
            admin,
            "EXPLAIN INSERT INTO role_statement_rows (title) VALUES ('allowed')",
            vec![],
        );

        // Assert
        assert!(result.is_ok());
    });
}
