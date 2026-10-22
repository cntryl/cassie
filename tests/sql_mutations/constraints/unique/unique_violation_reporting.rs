use cassie::app::{Cassie, CassieError, CassieSession};

use super::support_sql as support;
use support::data_dir;

fn start(label: &str) -> (Cassie, CassieSession, String) {
    let path = data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session, path)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

fn violated_constraint(
    result: Result<cassie::executor::QueryResult, CassieError>,
) -> Option<String> {
    if let Err(CassieError::UniqueViolation { constraint, .. }) = result {
        Some(constraint)
    } else {
        None
    }
}

#[test]
fn should_report_unique_index_violation_as_unique_violation() {
    // Arrange
    let (cassie, session, path) = start("unique-index-violation-sqlstate");
    run(
        &cassie,
        &session,
        "CREATE TABLE b (id INT PRIMARY KEY, email TEXT)",
    );
    run(
        &cassie,
        &session,
        "CREATE UNIQUE INDEX b_email ON b (email)",
    );
    run(
        &cassie,
        &session,
        "INSERT INTO b (id, email) VALUES (1, 'a@x')",
    );

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO b (id, email) VALUES (2, 'a@x')",
        vec![],
    );

    // Assert
    assert_eq!(violated_constraint(duplicate).as_deref(), Some("b_email"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_declared_unique_constraint_name() {
    // Arrange
    let (cassie, session, path) = start("unique-declared-name");
    run(
        &cassie,
        &session,
        "CREATE TABLE nn (id INT PRIMARY KEY, email TEXT, CONSTRAINT my_email_key UNIQUE (email))",
    );
    run(
        &cassie,
        &session,
        "INSERT INTO nn (id, email) VALUES (1, 'a@x')",
    );

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO nn (id, email) VALUES (2, 'a@x')",
        vec![],
    );

    // Assert
    assert_eq!(
        violated_constraint(duplicate).as_deref(),
        Some("my_email_key")
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_declared_primary_key_name() {
    // Arrange
    let (cassie, session, path) = start("primary-key-declared-name");
    run(
        &cassie,
        &session,
        "CREATE TABLE pk_named (id INT, CONSTRAINT my_pk PRIMARY KEY (id))",
    );
    run(&cassie, &session, "INSERT INTO pk_named (id) VALUES (1)");

    // Act
    let duplicate = cassie.execute_sql(&session, "INSERT INTO pk_named (id) VALUES (1)", vec![]);

    // Assert
    assert_eq!(violated_constraint(duplicate).as_deref(), Some("my_pk"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_unique_violation_given_conflict_outside_on_conflict_target() {
    // Arrange
    let (cassie, session, path) = start("unique-violation-outside-conflict-target");
    run(
        &cassie,
        &session,
        "CREATE TABLE oc (id INT PRIMARY KEY, email TEXT)",
    );
    run(
        &cassie,
        &session,
        "CREATE UNIQUE INDEX oc_email ON oc (email)",
    );
    run(
        &cassie,
        &session,
        "INSERT INTO oc (id, email) VALUES (1, 'a@x')",
    );

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO oc (id, email) VALUES (2, 'a@x') ON CONFLICT (id) DO NOTHING",
        vec![],
    );

    // Assert
    assert_eq!(violated_constraint(duplicate).as_deref(), Some("oc_email"));
    let _ = std::fs::remove_dir_all(path);
}
