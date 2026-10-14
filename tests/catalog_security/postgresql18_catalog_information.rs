//! PostgreSQL 18 catalog information behavior tracked by #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_list_created_relations_through_pg_tables() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-tables-created-relation",
        &[
            "CREATE SCHEMA issue_817_tables",
            "CREATE TABLE issue_817_tables.visible (id INTEGER)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, tablename FROM pg_catalog.pg_tables \
             WHERE schemaname = 'issue_817_tables' AND tablename = 'visible'",
        )
        .expect("query pg_tables for the created relation");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_tables".to_owned()),
            Value::String("visible".to_owned()),
        ]]
    );
}

#[test]
fn should_report_exact_postgresql_value_sizes() {
    // Arrange
    let fixture = sql_fixture("issue-817-column-size-values", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_column_size(CAST(0 AS INTEGER)), \
                    pg_catalog.pg_column_size(CAST(0 AS BIGINT)), \
                    pg_catalog.pg_column_size(CAST('foo' AS TEXT))",
        )
        .expect("query PostgreSQL datum sizes");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(4), Value::Int64(8), Value::Int64(7)]]
    );
}

#[test]
fn should_resolve_serial_sequence_from_relation_column() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-get-serial-sequence",
        &[
            "CREATE SCHEMA issue_817_serial",
            "CREATE TABLE issue_817_serial.items (id SERIAL PRIMARY KEY)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_get_serial_sequence( \
                'issue_817_serial.items', 'id')",
        )
        .expect("resolve the owned serial sequence");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(
            "issue_817_serial.items_id_seq".to_owned()
        )]]
    );
}

#[test]
fn should_format_integer_byte_sizes_using_postgresql_units() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-pretty", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_size_pretty(CAST(1024 AS BIGINT))")
        .expect("format the byte count");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("1024 bytes".to_owned())]]
    );
}
