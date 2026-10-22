use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use super::support_sql as support;

pub(super) fn open(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
    let path = support::data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in statements {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }
    (cassie, session, path)
}

pub(super) fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect(sql)
        .rows()
        .to_vec()
}

#[test]
fn should_decode_doubled_quotes_in_check_literals() {
    // Arrange
    let (cassie, session, path) = open(
            "check_doubled_quote_literal",
            &[
                "CREATE TABLE check_quote_eq (id INT NOT NULL, b TEXT CHECK (b = 'it''s'))",
                "CREATE TABLE check_quote_ne (id INT NOT NULL, b TEXT, CONSTRAINT not_its CHECK (b <> 'it''s'))",
            ],
        );

    // Act
    let only_legal_value = cassie.execute_sql(
        &session,
        "INSERT INTO check_quote_eq (id, b) VALUES (1, 'it''s')",
        vec![],
    );
    let forbidden_value = cassie.execute_sql(
        &session,
        "INSERT INTO check_quote_ne (id, b) VALUES (1, 'it''s')",
        vec![],
    );
    let two_quote_value = cassie.execute_sql(
        &session,
        "INSERT INTO check_quote_eq (id, b) VALUES (2, 'it''''s')",
        vec![],
    );

    // Assert
    assert!(only_legal_value.is_ok(), "the CHECK value itself must pass");
    assert!(
        forbidden_value.is_err(),
        "the forbidden value must be rejected"
    );
    assert!(
        two_quote_value.is_err(),
        "a value with two quotes differs from the CHECK value"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_store_doubled_quote_default_as_one_quote() {
    // Arrange
    let (cassie, session, path) = open(
        "default_doubled_quote_literal",
        &[
            "CREATE TABLE default_quote (id INT NOT NULL, a TEXT DEFAULT 'it''s')",
            "INSERT INTO default_quote (id) VALUES (1)",
        ],
    );

    // Act
    let stored = rows(
        &cassie,
        &session,
        "SELECT a FROM default_quote WHERE a = 'it''s'",
    );
    let column_default = rows(
            &cassie,
            &session,
            "SELECT column_default FROM information_schema.columns WHERE table_name = 'default_quote' AND column_name = 'a'",
        );

    // Assert
    assert_eq!(stored, vec![vec![Value::String("it's".to_string())]]);
    assert_eq!(
        column_default,
        vec![vec![Value::String("'it''s'".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_check_comparing_against_a_non_constant() {
    // Arrange
    let (cassie, session, path) = open(
        "check_non_constant_operand",
        &["CREATE TABLE check_alter_target (lo INT, hi INT)"],
    );

    // Act
    let column_reference = cassie.execute_sql(
        &session,
        "CREATE TABLE check_columns (id INT NOT NULL, lo INT, hi INT, CHECK (lo < hi))",
        vec![],
    );
    let quoted_column = cassie.execute_sql(
        &session,
        "CREATE TABLE check_quoted_column (lo INT, hi INT CHECK (hi > \"lo\"))",
        vec![],
    );
    let concatenation = cassie.execute_sql(
        &session,
        "CREATE TABLE check_concat (b TEXT CHECK (b = 'a' || 'b'))",
        vec![],
    );
    let altered = cassie.execute_sql(
        &session,
        "ALTER TABLE check_alter_target ADD CONSTRAINT lo_below_hi CHECK (lo < hi)",
        vec![],
    );

    // Assert
    assert!(
        column_reference.is_err(),
        "a CHECK whose right side is a column must not be stored as a string"
    );
    assert!(quoted_column.is_err(), "a quoted identifier is a column");
    assert!(concatenation.is_err(), "an expression is not a constant");
    assert!(altered.is_err(), "ADD CONSTRAINT must reject it too");
    assert!(!cassie.catalog.exists("check_columns"));
    assert!(!cassie.catalog.exists("check_quoted_column"));
    assert!(!cassie.catalog.exists("check_concat"));
    let _ = std::fs::remove_dir_all(path);
}
