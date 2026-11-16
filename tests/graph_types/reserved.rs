//! The reserved schema's DDL guards and the user-owned bootstrap conflict.

use crate::fixture::started;
use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;

fn blocked(label: &str, statement: &str) -> String {
    let fixture = started(label, &[]);
    fixture.sqlstate(statement)
}

#[test]
fn should_block_dropping_the_reserved_schema() {
    // Arrange
    let statement = "DROP SCHEMA cassie_graph";

    // Act
    let state = blocked("graph_reserved_drop", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_block_dropping_the_reserved_schema_with_cascade() {
    // Arrange
    let statement = "DROP SCHEMA cassie_graph CASCADE";

    // Act
    let state = blocked("graph_reserved_cascade", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_block_renaming_the_reserved_schema() {
    // Arrange
    let statement = "ALTER SCHEMA cassie_graph RENAME TO other";

    // Act
    let state = blocked("graph_reserved_rename", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_block_creating_a_type_in_the_reserved_schema() {
    // Arrange
    let statement = "CREATE TYPE cassie_graph.extra AS (a int)";

    // Act
    let state = blocked("graph_reserved_create_type", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_block_creating_a_table_in_the_reserved_schema() {
    // Arrange
    let statement = "CREATE TABLE cassie_graph.extra (a int)";

    // Act
    let state = blocked("graph_reserved_create_table", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_block_dropping_a_reserved_type_even_with_cascade() {
    // Arrange
    let statement = "DROP TYPE cassie_graph.graph_path CASCADE";

    // Act
    let state = blocked("graph_reserved_drop_type", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_block_renaming_a_reserved_type() {
    // Arrange
    let statement = "ALTER TYPE cassie_graph.graph_path RENAME TO moved";

    // Act
    let state = blocked("graph_reserved_rename_type", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_block_adding_an_attribute_to_a_reserved_type() {
    // Arrange
    let statement = "ALTER TYPE cassie_graph.graph_path ADD ATTRIBUTE extra int";

    // Act
    let state = blocked("graph_reserved_alter_type", statement);

    // Assert
    assert_eq!(state, "42501");
}

#[test]
fn should_keep_the_reserved_types_after_a_blocked_cascade_drop() {
    // Arrange
    let fixture = started("graph_reserved_intact", &[]);
    let _ = fixture.execute("DROP SCHEMA cassie_graph CASCADE");

    // Act
    let rows = fixture.rows("SELECT typname FROM pg_type WHERE typname = 'graph_path'");

    // Assert
    assert_eq!(rows.len(), 1);
}

fn unstarted() -> (Cassie, cassie::app::CassieSession) {
    let config = crate::support_sql::with_test_root_password(CassieRuntimeConfig::default());
    let cassie = Cassie::new_in_memory_with_config(config).expect("in-memory Cassie");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

#[test]
fn should_fail_bootstrap_with_42710_when_the_user_owns_the_schema() {
    // Arrange
    let (cassie, session) = unstarted();
    cassie
        .execute_sql(&session, "CREATE SCHEMA cassie_graph", Vec::new())
        .expect("user schema");

    // Act
    let error = cassie
        .ensure_graph_schema("postgres")
        .expect_err("conflict");

    // Assert
    assert_eq!(error.sql_state(), "42710");
}

#[test]
fn should_leave_the_user_schema_untouched_when_bootstrap_conflicts() {
    // Arrange
    let (cassie, session) = unstarted();
    cassie
        .execute_sql(&session, "CREATE SCHEMA cassie_graph", Vec::new())
        .expect("user schema");

    // Act
    let _ = cassie.ensure_graph_schema("postgres");

    // Assert
    let types = cassie
        .catalog
        .get_composite_type("postgres.cassie_graph.graph_path");
    assert!(types.is_none());
    assert!(cassie.catalog.namespace_exists("postgres.cassie_graph"));
}

#[test]
fn should_bootstrap_after_the_user_removes_the_conflicting_schema() {
    // Arrange
    let (cassie, session) = unstarted();
    cassie
        .execute_sql(&session, "CREATE SCHEMA cassie_graph", Vec::new())
        .expect("user schema");
    let _ = cassie.ensure_graph_schema("postgres");
    cassie
        .execute_sql(&session, "DROP SCHEMA cassie_graph", Vec::new())
        .expect("user removes the schema");

    // Act
    let outcome = cassie.ensure_graph_schema("postgres");

    // Assert
    assert!(outcome.is_ok());
    assert!(cassie
        .catalog
        .get_composite_type("postgres.cassie_graph.graph_path")
        .is_some());
}

#[test]
fn should_drop_an_empty_database_that_holds_the_reserved_schema() {
    // Arrange
    let fixture = started("graph_reserved_drop_database", &[]);
    let password = crate::support_sql::test_root_password();
    let root = fixture
        .cassie
        .authenticate_role("root", Some(password), None)
        .expect("root login");
    fixture
        .cassie
        .execute_sql(&root, "CREATE DATABASE scratch", Vec::new())
        .expect("create database");

    // Act
    let dropped = fixture
        .cassie
        .execute_sql(&root, "DROP DATABASE scratch", Vec::new());

    // Assert
    assert!(dropped.is_ok());
}

#[test]
fn should_bootstrap_the_reserved_schema_in_a_new_database() {
    // Arrange
    let fixture = started("graph_reserved_new_database", &[]);
    let password = crate::support_sql::test_root_password();
    let root = fixture
        .cassie
        .authenticate_role("root", Some(password), None)
        .expect("root login");
    fixture
        .cassie
        .execute_sql(&root, "CREATE DATABASE scratch", Vec::new())
        .expect("create database");

    // Act
    let published = fixture
        .cassie
        .catalog
        .get_composite_type("scratch.cassie_graph.graph_path");

    // Assert
    assert!(published.is_some());
}

#[test]
fn should_give_a_restored_database_its_own_reserved_schema() {
    // Arrange
    let fixture = started("graph_reserved_restore", &[]);
    let mut backup = fixture
        .cassie
        .begin_database_backup("postgres")
        .expect("backup");
    let mut restore = fixture
        .cassie
        .begin_database_restore("restored")
        .expect("restore");
    while let Some(chunk) = backup.next_chunk().expect("chunk") {
        restore.push_chunk(&chunk).expect("push");
    }

    // Act
    restore.finish().expect("finish");

    // Assert
    let source = fixture
        .cassie
        .catalog
        .get_composite_type("postgres.cassie_graph.graph_path")
        .expect("source type");
    let target = fixture
        .cassie
        .catalog
        .get_composite_type("restored.cassie_graph.graph_path")
        .expect("restored type");
    assert_ne!(source.object_id, target.object_id);
}

#[test]
fn should_block_every_other_object_kind_in_the_reserved_schema() {
    // Arrange
    let fixture = started(
        "graph_reserved_objects",
        &[
            "CREATE TABLE plain (id bigint PRIMARY KEY, name text)",
            "CREATE TABLE links (id bigint PRIMARY KEY, a bigint NOT NULL, b bigint NOT NULL)",
        ],
    );
    let statements = [
        "CREATE VIEW cassie_graph.v AS SELECT id FROM plain",
        "CREATE MATERIALIZED VIEW cassie_graph.mv AS SELECT id FROM plain",
        "CREATE SEQUENCE cassie_graph.s",
        "CREATE FUNCTION cassie_graph.key_part(x INT) RETURNS INT AS \"x\"",
        "CREATE FUNCTION cassie_graph.mine(x INT) RETURNS INT AS \"x\"",
        "CREATE PROCEDURE cassie_graph.p() AS \"SELECT id FROM plain\"",
        "CREATE TABLE cassie_graph.copy AS SELECT id FROM plain",
        "ALTER TABLE plain RENAME TO cassie_graph.plain",
        "CREATE PROPERTY GRAPH cassie_graph.g \
           VERTEX TABLES (plain AS p KEY (id) LABEL p PROPERTIES (name)) \
           EDGE TABLES (links AS l KEY (id) \
             SOURCE KEY (a) REFERENCES p (id) DESTINATION KEY (b) REFERENCES p (id) \
             LABEL l PROPERTIES (id))",
        "COMMENT ON TYPE cassie_graph.graph_path IS 'mine'",
        "ALTER TYPE cassie_graph.graph_key_part DROP ATTRIBUTE text_value",
        "ALTER TYPE cassie_graph.graph_key_part ALTER ATTRIBUTE type_tag TYPE int",
        "ALTER TYPE cassie_graph.graph_path SET SCHEMA public",
    ];

    // Act
    let accepted = statements
        .iter()
        .filter(|statement| fixture.execute(statement).is_ok())
        .copied()
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(accepted, Vec::<&str>::new());
}
