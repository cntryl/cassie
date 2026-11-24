use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use super::support_sql as support;

fn start() -> (Cassie, CassieSession) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).expect(sql);
}

fn sql_state(cassie: &Cassie, session: &CassieSession, sql: &str) -> &'static str {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(_) => "00000",
        Err(error) => error.sql_state(),
    }
}

fn row_count(cassie: &Cassie, session: &CassieSession, table: &str) -> usize {
    cassie
        .execute_sql(session, &format!("SELECT * FROM {table}"), vec![])
        .expect("read rows")
        .rows()
        .len()
}

#[test]
fn should_accept_quoted_column_in_table_level_check() {
    // Arrange
    let (cassie, session) = start();

    // Act
    let created = cassie.execute_sql(
        &session,
        r#"CREATE TABLE quoted_check_table (n INT, CHECK ("n" > 0))"#,
        vec![],
    );

    // Assert
    assert!(created.is_ok(), "quoted table-level CHECK was rejected");
}

#[test]
fn should_accept_operator_characters_inside_quoted_check_column() {
    // Arrange
    let (cassie, session) = start();

    // Act
    let created = cassie.execute_sql(
        &session,
        r#"CREATE TABLE quoted_check_operator ("N>=U" INT, CHECK ("N>=U">0))"#,
        vec![],
    );

    // Assert
    assert!(
        created.is_ok(),
        "operator-bearing quoted CHECK was rejected"
    );
}

#[test]
fn should_accept_quoted_column_in_column_level_check() {
    // Arrange
    let (cassie, session) = start();

    // Act
    let created = cassie.execute_sql(
        &session,
        r#"CREATE TABLE quoted_check_inline ("Qty" INT CHECK ("Qty" > 0))"#,
        vec![],
    );

    // Assert
    assert!(created.is_ok(), "quoted column-level CHECK was rejected");
}

#[test]
fn should_reject_zero_for_quoted_table_level_check() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_zero (n INT, CHECK ("n" > 0))"#,
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        "INSERT INTO quoted_check_zero (n) VALUES (0)",
    );

    // Assert
    assert_eq!(state, "23514");
}

#[test]
fn should_accept_positive_value_for_quoted_table_level_check() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_positive (n INT, CHECK ("n" > 0))"#,
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        "INSERT INTO quoted_check_positive (n) VALUES (5)",
    );

    // Assert
    assert_eq!(state, "00000");
}

#[test]
fn should_accept_null_for_quoted_table_level_check() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_null (n INT, CHECK ("n" > 0))"#,
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        "INSERT INTO quoted_check_null (n) VALUES (NULL)",
    );

    // Assert
    assert_eq!(state, "00000");
}

#[test]
fn should_reject_zero_for_operator_named_quoted_column() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_operator_zero ("N>=U" INT, CHECK ("N>=U">0))"#,
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        r#"INSERT INTO quoted_check_operator_zero ("N>=U") VALUES (0)"#,
    );

    // Assert
    assert_eq!(state, "23514");
}

#[test]
fn should_reject_zero_for_quoted_column_level_check() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_inline_zero ("Qty" INT CHECK ("Qty" > 0))"#,
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        r#"INSERT INTO quoted_check_inline_zero ("Qty") VALUES (0)"#,
    );

    // Assert
    assert_eq!(state, "23514");
}

#[test]
fn should_keep_prior_rows_when_quoted_check_rejects_a_write() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_atomic (n INT, CHECK ("n" > 0))"#,
    );
    run(
        &cassie,
        &session,
        "INSERT INTO quoted_check_atomic (n) VALUES (3)",
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        "INSERT INTO quoted_check_atomic (n) VALUES (4), (0)",
    );

    // Assert
    assert_eq!(state, "23514");
    assert_eq!(row_count(&cassie, &session, "quoted_check_atomic"), 1);
}

#[test]
fn should_attach_check_to_the_exact_case_column() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_case ("Qty" INT, qty INT, CHECK ("Qty" > 0))"#,
    );

    // Act
    let constrained = sql_state(
        &cassie,
        &session,
        r#"INSERT INTO quoted_check_case ("Qty", qty) VALUES (0, 1)"#,
    );
    let unconstrained = sql_state(
        &cassie,
        &session,
        r#"INSERT INTO quoted_check_case ("Qty", qty) VALUES (1, 0)"#,
    );

    // Assert
    assert_eq!(constrained, "23514");
    assert_eq!(unconstrained, "00000");
}

#[test]
fn should_resolve_escaped_double_quote_in_check_column() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        r#"CREATE TABLE quoted_check_escape ("a""b" INT, CHECK ("a""b" > 0))"#,
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        r#"INSERT INTO quoted_check_escape ("a""b") VALUES (0)"#,
    );

    // Assert
    assert_eq!(state, "23514");
}

#[test]
fn should_reject_quoted_check_column_that_does_not_exist() {
    // Arrange
    let (cassie, session) = start();

    // Act
    let created = cassie.execute_sql(
        &session,
        r#"CREATE TABLE quoted_check_missing (n INT, CHECK ("missing" > 0))"#,
        vec![],
    );

    // Assert
    assert!(created.is_err(), "unknown quoted CHECK column was accepted");
}

#[test]
fn should_reject_quoted_check_column_with_different_case() {
    // Arrange
    let (cassie, session) = start();

    // Act
    let created = cassie.execute_sql(
        &session,
        r#"CREATE TABLE quoted_check_case_mismatch (n INT, CHECK ("N" > 0))"#,
        vec![],
    );

    // Assert
    assert!(
        created.is_err(),
        "case-mismatched quoted CHECK was accepted"
    );
}

#[test]
fn should_leave_no_relation_after_mismatched_quoted_check_column() {
    // Arrange
    let (cassie, session) = start();
    let _ = cassie.execute_sql(
        &session,
        r#"CREATE TABLE quoted_check_leftover (n INT, CHECK ("N" > 0))"#,
        vec![],
    );

    // Act
    let selected = cassie.execute_sql(&session, "SELECT * FROM quoted_check_leftover", vec![]);

    // Assert
    assert!(selected.is_err(), "failed DDL left a relation behind");
}

#[test]
fn should_keep_enforcing_unquoted_check() {
    // Arrange
    let (cassie, session) = start();
    run(
        &cassie,
        &session,
        "CREATE TABLE quoted_check_unquoted (qty INT, CHECK (qty > 0))",
    );

    // Act
    let state = sql_state(
        &cassie,
        &session,
        "INSERT INTO quoted_check_unquoted (qty) VALUES (0)",
    );

    // Assert
    assert_eq!(state, "23514");
}

#[test]
fn should_enforce_quoted_check_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("quoted-check-restart");
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        run(
            &cassie,
            &session,
            r#"CREATE TABLE quoted_check_restart ("Qty" INT, qty INT, CHECK ("Qty" > 0))"#,
        );
        run(
            &cassie,
            &session,
            r#"INSERT INTO quoted_check_restart ("Qty", qty) VALUES (2, 0)"#,
        );
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    cassie.startup().expect("restart Cassie");
    let session = cassie.create_session("tester", None);
    let constrained = sql_state(
        &cassie,
        &session,
        r#"INSERT INTO quoted_check_restart ("Qty", qty) VALUES (0, 1)"#,
    );
    let unconstrained = sql_state(
        &cassie,
        &session,
        r#"INSERT INTO quoted_check_restart ("Qty", qty) VALUES (1, 0)"#,
    );

    // Assert
    assert_eq!(constrained, "23514");
    assert_eq!(unconstrained, "00000");
    let rows = cassie
        .execute_sql(
            &session,
            r#"SELECT "Qty" FROM quoted_check_restart ORDER BY "Qty""#,
            vec![],
        )
        .expect("read rows");
    assert_eq!(rows.rows()[0][0], Value::Int64(1));
    let _ = std::fs::remove_dir_all(path);
}
