//! PostgreSQL 18.6 relation catalog and system size contracts for #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

fn size_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE TABLE issue_817_size_probe (id INTEGER, payload TEXT)",
            "CREATE INDEX issue_817_size_probe_idx ON issue_817_size_probe (id)",
            "INSERT INTO issue_817_size_probe VALUES \
                (1, 'first'), (2, 'second'), (3, 'third')",
        ],
    );
    fixture.cassie.startup().expect("startup");
    fixture
}

#[test]
fn should_expose_pg_tables_row_metadata() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-tables-row-contract",
        &[
            "CREATE SCHEMA issue_817_table_contract",
            "CREATE TABLE issue_817_table_contract.visible (id INTEGER)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, tablename, \
                    tableowner = current_user AS owner_is_current_user, tablespace, \
                    hasindexes, hasrules, hastriggers, rowsecurity \
             FROM pg_catalog.pg_tables \
             WHERE schemaname = 'issue_817_table_contract' AND tablename = 'visible'",
        )
        .expect("read all selected pg_tables metadata for the created table");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("schemaname", 19),
            ("tablename", 19),
            ("owner_is_current_user", 16),
            ("tablespace", 19),
            ("hasindexes", 16),
            ("hasrules", 16),
            ("hastriggers", 16),
            ("rowsecurity", 16),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_table_contract".to_owned()),
            Value::String("visible".to_owned()),
            Value::Bool(true),
            Value::Null,
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_expose_pg_views_definition_metadata() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-views-row-contract",
        &[
            "CREATE SCHEMA issue_817_view_contract",
            "CREATE VIEW issue_817_view_contract.answer AS SELECT 42 AS answer",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, viewname, \
                    viewowner = current_user AS owner_is_current_user, definition \
             FROM pg_catalog.pg_views \
             WHERE schemaname = 'issue_817_view_contract' AND viewname = 'answer'",
        )
        .expect("read the created view's catalog definition and owner");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("schemaname", 19),
            ("viewname", 19),
            ("owner_is_current_user", 16),
            ("definition", 25),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_view_contract".to_owned()),
            Value::String("answer".to_owned()),
            Value::Bool(true),
            Value::String(" SELECT 42 AS answer;".to_owned()),
        ]]
    );
}

#[test]
fn should_keep_created_catalog_rows_visible_without_schema_usage() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-catalog-visibility-without-schema-usage",
        &[
            "CREATE ROLE issue_817_catalog_reader",
            "CREATE SCHEMA issue_817_private_catalog",
            "CREATE TABLE issue_817_private_catalog.hidden_table (id INTEGER)",
            "CREATE INDEX issue_817_hidden_idx \
             ON issue_817_private_catalog.hidden_table (id)",
            "CREATE VIEW issue_817_private_catalog.hidden_view AS SELECT 42 AS answer",
            "REVOKE ALL ON SCHEMA issue_817_private_catalog FROM PUBLIC",
            "REVOKE ALL ON TABLE issue_817_private_catalog.hidden_table FROM PUBLIC",
            "REVOKE ALL ON TABLE issue_817_private_catalog.hidden_view FROM PUBLIC",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let reader = fixture
        .cassie
        .create_session("issue_817_catalog_reader", None);

    // Act
    let selected = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT \
                (SELECT count(*) FROM pg_catalog.pg_tables \
                 WHERE schemaname = 'issue_817_private_catalog' \
                   AND tablename = 'hidden_table'), \
                (SELECT count(*) FROM pg_catalog.pg_views \
                 WHERE schemaname = 'issue_817_private_catalog' \
                   AND viewname = 'hidden_view'), \
                (SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE schemaname = 'issue_817_private_catalog' \
                   AND tablename = 'hidden_table' \
                   AND indexname = 'issue_817_hidden_idx')",
            vec![],
        )
        .expect("read catalog rows for objects in a schema without USAGE");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(1), Value::Int64(1), Value::Int64(1),]]
    );
}

include!("../sequences/size_information.rs");

include!("../sizes/size_information.rs");
