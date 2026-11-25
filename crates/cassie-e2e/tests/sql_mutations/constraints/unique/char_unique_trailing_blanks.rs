use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::Value;

use super::support_sql as support;
use support::data_dir;

fn start(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
    let path = data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in statements {
        cassie
            .execute_sql(&session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }
    (cassie, session, path)
}

fn is_unique_violation(result: &Result<cassie::executor::QueryResult, CassieError>) -> bool {
    matches!(result, Err(CassieError::UniqueViolation { .. }))
}

#[test]
fn should_treat_char_values_differing_in_trailing_blanks_as_duplicates() {
    // Arrange
    let (cassie, session, path) = start(
        "char-unique-trailing-blanks",
        &[
            "CREATE TABLE f6 (id INTEGER PRIMARY KEY, c CHAR(3) UNIQUE)",
            "CREATE TABLE f7 (id INTEGER PRIMARY KEY, c CHAR(3))",
            "CREATE UNIQUE INDEX f7_c ON f7 (c)",
            "INSERT INTO f6 (id, c) VALUES (1, 'x')",
            "INSERT INTO f7 (id, c) VALUES (1, 'x  ')",
        ],
    );

    // Act
    let constraint_duplicate =
        cassie.execute_sql(&session, "INSERT INTO f6 (id, c) VALUES (2, 'x  ')", vec![]);
    let index_duplicate =
        cassie.execute_sql(&session, "INSERT INTO f7 (id, c) VALUES (2, 'x')", vec![]);
    let padded_past_length = cassie.execute_sql(
        &session,
        "INSERT INTO f6 (id, c) VALUES (3, 'yz    ')",
        vec![],
    );
    let matched = cassie
        .execute_sql(&session, "SELECT id, c FROM f7 WHERE c = 'x'", vec![])
        .expect("select f7")
        .rows()
        .to_vec();

    // Assert
    assert!(
        is_unique_violation(&constraint_duplicate),
        "'x  ' equals 'x' in CHAR(3)"
    );
    assert!(
        is_unique_violation(&index_duplicate),
        "'x' equals 'x  ' in CHAR(3)"
    );
    assert!(
        padded_past_length.is_ok(),
        "trailing blanks beyond the length are insignificant"
    );
    assert_eq!(
        matched,
        vec![vec![Value::Int64(1), Value::String("x".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}
