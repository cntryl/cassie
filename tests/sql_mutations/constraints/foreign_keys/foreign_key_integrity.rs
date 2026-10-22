use cassie::app::{Cassie, CassieSession};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

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

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("{sql}: {error}"))
        .rows()
        .to_vec()
}

#[test]
fn should_refuse_to_drop_unique_index_a_foreign_key_depends_on() {
    // Arrange
    let (cassie, session, path) = start("fk-unique-index-drop");
    run(
        &cassie,
        &session,
        "CREATE TABLE p (pk INT PRIMARY KEY, id INT)",
    );
    run(&cassie, &session, "CREATE UNIQUE INDEX p_id_uniq ON p (id)");
    run(
            &cassie,
            &session,
            "CREATE TABLE c (cid INT, pid INT, CONSTRAINT cfk FOREIGN KEY (pid) REFERENCES p(id) ON DELETE CASCADE)",
        );

    // Act
    let dropped = cassie.execute_sql(&session, "DROP INDEX p_id_uniq ON p", vec![]);

    // Assert
    assert!(
        dropped.is_err(),
        "the FK's unique index must not be droppable"
    );
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO p (pk, id) VALUES (1, 100), (2, 100)",
        vec![],
    );
    assert!(duplicate.is_err(), "p.id must stay unique");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_copy_that_changes_a_referenced_parent_key() {
    // Arrange
    let (cassie, session, path) = start("fk-copy-parent-key");
    run(&cassie, &session, "CREATE TABLE p (id INT PRIMARY KEY)");
    run(
        &cassie,
        &session,
        "CREATE TABLE c (cid INT, pid INT REFERENCES p(id))",
    );
    let copy = CopyStatement {
        table: "p".to_string(),
        columns: vec!["_id".to_string(), "id".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };
    cassie
        .copy_from_csv_stdin(&session, &copy, b"row-a,1\n")
        .expect("seed parent");
    run(&cassie, &session, "INSERT INTO c (cid, pid) VALUES (10, 1)");

    // Act
    let rekeyed = cassie.copy_from_csv_stdin(&session, &copy, b"row-a,2\n");

    // Assert
    assert!(
        rekeyed.is_err(),
        "COPY must not orphan c's reference to p.id = 1"
    );
    assert_eq!(
        rows(&cassie, &session, "SELECT id FROM p"),
        vec![vec![Value::Int64(1)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_accept_child_timestamps_equal_to_the_parent_key() {
    // Arrange
    let (cassie, session, path) = start("fk-timestamp-key");
    run(
        &cassie,
        &session,
        "CREATE TABLE pt (at TIMESTAMP PRIMARY KEY)",
    );
    run(
        &cassie,
        &session,
        "CREATE TABLE ct (id INT, at TIMESTAMP REFERENCES pt(at))",
    );
    run(
        &cassie,
        &session,
        "INSERT INTO pt (at) VALUES ('2024-01-01 00:00:00')",
    );

    // Act
    let same_literal = cassie.execute_sql(
        &session,
        "INSERT INTO ct (id, at) VALUES (1, '2024-01-01 00:00:00')",
        vec![],
    );
    let rfc3339 = cassie.execute_sql(
        &session,
        "INSERT INTO ct (id, at) VALUES (2, '2024-01-01T00:00:00Z')",
        vec![],
    );

    // Assert
    assert!(
        same_literal.is_ok(),
        "the parent's own literal satisfies the FK"
    );
    assert!(
        rfc3339.is_ok(),
        "an equal RFC 3339 timestamp satisfies the FK"
    );
    let _ = std::fs::remove_dir_all(path);
}

const NODE_DDL: &str = "CREATE TABLE node (id INT PRIMARY KEY, parent_id INT, CONSTRAINT nfk FOREIGN KEY (parent_id) REFERENCES node(id))";

#[test]
fn should_declare_self_referencing_foreign_key_at_create_table() {
    // Arrange
    let (cassie, session, path) = start("fk-self-reference-declare");

    // Act
    let created = cassie.execute_sql(&session, NODE_DDL, vec![]);

    // Assert
    assert!(created.is_ok(), "self-referencing FK at CREATE TABLE");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_accept_self_pointing_row_given_self_referencing_foreign_key() {
    // Arrange
    let (cassie, session, path) = start("fk-self-reference-insert");
    run(&cassie, &session, NODE_DDL);

    // Act
    let root = cassie.execute_sql(
        &session,
        "INSERT INTO node (id, parent_id) VALUES (1, 1)",
        vec![],
    );

    // Assert
    assert!(
        root.is_ok(),
        "a self-pointing root row satisfies its own FK"
    );
    assert!(cassie
        .execute_sql(
            &session,
            "INSERT INTO node (id, parent_id) VALUES (3, 99)",
            vec![],
        )
        .is_err());
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_delete_row_referenced_only_by_itself() {
    // Arrange
    let (cassie, session, path) = start("fk-self-reference-delete");
    run(&cassie, &session, NODE_DDL);
    run(
        &cassie,
        &session,
        "INSERT INTO node (id, parent_id) VALUES (1, 1)",
    );

    // Act
    let deleted = cassie.execute_sql(&session, "DELETE FROM node WHERE id = 1", vec![]);

    // Assert
    assert!(
        deleted.is_ok(),
        "a row referenced only by itself is deletable"
    );
    assert_eq!(
        rows(&cassie, &session, "SELECT id FROM node"),
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    let _ = std::fs::remove_dir_all(path);
}
