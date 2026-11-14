//! `pg_type` rows for table row types stay truthful per schema and per
//! database (#817). Expected shapes come from a PostgreSQL 18.6 oracle.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const SETUP: &[&str] = &[
    "CREATE SCHEMA s1",
    "CREATE TABLE t1 (id INT PRIMARY KEY)",
    "CREATE TABLE s1.t1 (id INT PRIMARY KEY)",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, SETUP)
}

#[test]
fn should_publish_one_row_type_per_table_when_two_schemas_share_a_table_name() {
    // Arrange
    let fixture = fixture("row_type_same_name_rows");

    // Act
    let counts = fixture
        .rows("SELECT count(*), count(DISTINCT oid) FROM pg_type WHERE typname IN ('t1', '_t1')");

    // Assert
    assert_eq!(counts, vec![vec![Value::Int64(4), Value::Int64(4)]]);
}

#[test]
fn should_publish_each_row_type_in_its_own_table_namespace() {
    // Arrange
    let fixture = fixture("row_type_same_name_namespace");
    let namespaces = fixture
        .rows("SELECT 'public'::regnamespace::oid::bigint, 's1'::regnamespace::oid::bigint")
        .remove(0);

    // Act
    let public_row =
        fixture.rows("SELECT typnamespace::bigint FROM pg_type WHERE oid = 'public.t1'::regtype");
    let schema_row =
        fixture.rows("SELECT typnamespace::bigint FROM pg_type WHERE oid = 's1.t1'::regtype");

    // Assert
    assert_eq!(public_row, vec![vec![namespaces[0].clone()]]);
    assert_eq!(schema_row, vec![vec![namespaces[1].clone()]]);
}

#[test]
fn should_point_each_row_type_at_its_own_table() {
    // Arrange
    let fixture = fixture("row_type_same_name_typrelid");
    let relations = fixture
        .rows("SELECT 'public.t1'::regclass::oid::bigint, 's1.t1'::regclass::oid::bigint")
        .remove(0);

    // Act
    let public_row =
        fixture.rows("SELECT typrelid::bigint FROM pg_type WHERE oid = 'public.t1'::regtype");
    let schema_row =
        fixture.rows("SELECT typrelid::bigint FROM pg_type WHERE oid = 's1.t1'::regtype");

    // Assert
    assert_eq!(public_row, vec![vec![relations[0].clone()]]);
    assert_eq!(schema_row, vec![vec![relations[1].clone()]]);
}

#[test]
fn should_not_publish_the_row_types_of_another_database() {
    // Arrange
    let fixture = fixture("row_type_database_scope");
    fixture
        .execute_as_root("CREATE DATABASE row_type_other")
        .expect("create the second database");
    let other = fixture
        .cassie
        .create_session("root", Some("row_type_other".to_string()));

    // Act
    let leaked = fixture
        .cassie
        .execute_sql(
            &other,
            "SELECT count(*) FROM pg_type WHERE typname IN ('t1', '_t1')",
            vec![],
        )
        .expect("read pg_type in the second database");

    // Assert
    assert_eq!(leaked.rows(), vec![vec![Value::Int64(0)]]);
}
