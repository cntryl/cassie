//! A non-root role cannot read an ungranted table through any statement kind (#831).

use cassie::app::{Cassie, CassieError, CassieSession};

fn run_as_reader(sql: &str) -> Result<cassie::executor::QueryResult, CassieError> {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let admin = cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("admin");
    for setup in [
        "CREATE TABLE secret_rows (title TEXT)",
        "INSERT INTO secret_rows VALUES ('classified')",
        "CREATE TABLE open_rows (title TEXT)",
        "CREATE VIEW secret_view AS SELECT title FROM secret_rows",
        "CREATE ROLE path_reader LOGIN PASSWORD 'path-secret'",
        "GRANT USAGE ON SCHEMA public TO path_reader",
        "GRANT SELECT, INSERT ON TABLE public.open_rows TO path_reader",
    ] {
        cassie.execute_sql(&admin, setup, vec![]).expect(setup);
    }
    let reader = cassie
        .authenticate_role("path_reader", Some("path-secret"), None)
        .expect("reader");
    run_chain(&cassie, &reader, sql)
}

/// Runs `;`-separated statements and returns the first failure, or the last success.
fn run_chain(
    cassie: &Cassie,
    session: &CassieSession,
    sql: &str,
) -> Result<cassie::executor::QueryResult, CassieError> {
    let mut last = None;
    for statement in sql.split(';') {
        last = Some(cassie.execute_sql(session, statement.trim(), vec![])?);
    }
    Ok(last.expect("at least one statement"))
}

#[test]
fn should_deny_select_on_an_ungranted_table() {
    // Arrange
    let sql = "SELECT title FROM secret_rows";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_explain_select_on_an_ungranted_table() {
    // Arrange
    let sql = "EXPLAIN SELECT title FROM secret_rows";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_explain_analyze_select_on_an_ungranted_table() {
    // Arrange
    let sql = "EXPLAIN ANALYZE SELECT title FROM secret_rows";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_executing_a_prepared_ungranted_read() {
    // Arrange
    let sql = "PREPARE read_secret AS SELECT title FROM secret_rows; EXECUTE read_secret";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_a_cursor_over_an_ungranted_table() {
    // Arrange
    let sql = "BEGIN; DECLARE secret_cursor CURSOR FOR SELECT title FROM secret_rows; FETCH 1 FROM secret_cursor";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_insert_select_from_an_ungranted_table() {
    // Arrange
    let sql = "INSERT INTO open_rows SELECT title FROM secret_rows";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_create_table_as_select_from_an_ungranted_table() {
    // Arrange
    let sql = "CREATE TABLE stolen AS SELECT title FROM secret_rows";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_a_cte_over_an_ungranted_table() {
    // Arrange
    let sql = "WITH s AS (SELECT title FROM secret_rows) SELECT title FROM s";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_a_scalar_subquery_over_an_ungranted_table() {
    // Arrange
    let sql = "SELECT (SELECT title FROM secret_rows LIMIT 1)";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_a_set_operation_branch_over_an_ungranted_table() {
    // Arrange
    let sql = "SELECT title FROM open_rows UNION ALL SELECT title FROM secret_rows";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_a_join_against_an_ungranted_table() {
    // Arrange
    let sql = "SELECT o.title FROM open_rows o JOIN secret_rows s ON o.title = s.title";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_a_view_that_was_not_granted() {
    // Arrange
    let sql = "SELECT title FROM secret_view";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_update_that_reads_an_ungranted_table() {
    // Arrange
    let sql = "UPDATE open_rows SET title = (SELECT title FROM secret_rows LIMIT 1)";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_delete_that_reads_an_ungranted_table() {
    // Arrange
    let sql = "DELETE FROM open_rows WHERE title IN (SELECT title FROM secret_rows)";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_an_exists_subquery_over_an_ungranted_table() {
    // Arrange
    let sql = "SELECT title FROM open_rows WHERE EXISTS (SELECT 1 FROM secret_rows)";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_a_lateral_subquery_over_an_ungranted_table() {
    // Arrange
    let sql =
        "SELECT o.title FROM open_rows o JOIN LATERAL (SELECT title FROM secret_rows) AS s ON TRUE";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}

#[test]
fn should_deny_the_table_shorthand_over_an_ungranted_table() {
    // Arrange
    let sql = "TABLE secret_rows";

    // Act
    let result = run_as_reader(sql);

    // Assert
    assert!(matches!(result, Err(CassieError::InsufficientPrivilege)));
}
