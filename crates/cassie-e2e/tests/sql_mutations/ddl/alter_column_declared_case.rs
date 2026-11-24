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
        run(&cassie, &session, sql);
    }
    (cassie, session, path)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

#[test]
fn should_attach_alter_column_changes_to_the_declared_column() {
    // Arrange
    let (cassie, session, path) = start(
        "alter-column-declared-case",
        &[
            "CREATE TABLE notes (Note TEXT, Email TEXT, Legacy TEXT, v INT)",
            "INSERT INTO notes (Email, v) VALUES ('a@x', 0)",
        ],
    );

    // Act
    let set_default = cassie.execute_sql(
        &session,
        "ALTER TABLE notes ALTER COLUMN note SET DEFAULT 'auto'",
        vec![],
    );
    let set_not_null = cassie.execute_sql(
        &session,
        "ALTER TABLE notes ALTER COLUMN email SET NOT NULL",
        vec![],
    );
    let defaulted = cassie.execute_sql(
        &session,
        "INSERT INTO notes (Email, v) VALUES ('b@x', 1)",
        vec![],
    );
    let missing_email = cassie.execute_sql(&session, "INSERT INTO notes (v) VALUES (2)", vec![]);
    run(
        &cassie,
        &session,
        "ALTER TABLE notes ALTER COLUMN EMAIL DROP NOT NULL",
    );
    run(
        &cassie,
        &session,
        "ALTER TABLE notes ALTER COLUMN NOTE DROP DEFAULT",
    );
    let relaxed = cassie.execute_sql(&session, "INSERT INTO notes (v) VALUES (3)", vec![]);
    let dropped_column =
        cassie.execute_sql(&session, "ALTER TABLE notes DROP COLUMN legacy", vec![]);
    let renamed_column = cassie.execute_sql(
        &session,
        "ALTER TABLE notes RENAME COLUMN NOTE TO memo",
        vec![],
    );
    let stored = cassie
        .execute_sql(
            &session,
            "SELECT memo, email, v FROM notes ORDER BY v",
            vec![],
        )
        .expect("select notes")
        .rows()
        .to_vec();

    // Assert
    assert!(set_default.is_ok(), "SET DEFAULT names the column Note");
    assert!(set_not_null.is_ok(), "no stored Email is NULL");
    assert!(defaulted.is_ok(), "the default attaches to Note");
    assert!(
        matches!(missing_email, Err(CassieError::NotNullViolation { .. })),
        "SET NOT NULL attaches to Email"
    );
    assert!(relaxed.is_ok(), "DROP NOT NULL and DROP DEFAULT attach too");
    assert!(
        dropped_column.is_ok(),
        "DROP COLUMN names the column Legacy"
    );
    assert!(
        renamed_column.is_ok(),
        "RENAME COLUMN names the column Note"
    );
    assert_eq!(
        stored,
        vec![
            vec![
                Value::Null,
                Value::String("a@x".to_string()),
                Value::Int64(0)
            ],
            vec![
                Value::String("auto".to_string()),
                Value::String("b@x".to_string()),
                Value::Int64(1),
            ],
            vec![Value::Null, Value::Null, Value::Int64(3)],
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}
