//! Column-authoritative tables in the catalogs (#1106): `pg_am`, `pg_class.relam`,
//! `pg_table_storage` and text EXPLAIN agree on each table's authoritative mode.

use crate::support_sql::{assert_explain_contains, explain_plan_text};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

const MIXED: &[&str] = &[
    "CREATE SCHEMA mixed",
    "CREATE TABLE mixed.row_default (id INT, label TEXT)",
    "CREATE TABLE mixed.row_heap (id INT, label TEXT) USING heap",
    "CREATE TABLE mixed.col_using (id INT, label TEXT) USING columnar",
    "CREATE TABLE mixed.col_with (id INT, label TEXT) WITH (storage = column_store)",
    "CREATE TABLE mixed.col_both (id INT, label TEXT) USING columnar WITH (storage = column_store)",
    "CREATE TABLE mixed.col_ctas USING columnar AS SELECT 1 AS id, 'a' AS label",
    "INSERT INTO mixed.row_default VALUES (1, 'row')",
    "INSERT INTO mixed.col_using VALUES (2, 'column')",
];

fn text_rows(fixture: &SqlFixture, sql: &str) -> Vec<Vec<String>> {
    fixture
        .rows(sql)
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| match value {
                    Value::String(text) => text,
                    Value::Null => "NULL".to_owned(),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

/// A fixture whose names are database-qualified, as under a started server:
/// schema reporting and `SET SCHEMA` need that identity.
fn started_fixture(label: &str, setup: &[&str]) -> SqlFixture {
    let mut fixture = sql_fixture(label, &[]);
    fixture.cassie.startup().expect("start Cassie");
    fixture.session = fixture.cassie.create_session("tester", None);
    for sql in setup {
        fixture
            .execute(sql)
            .unwrap_or_else(|error| panic!("setup failed: {sql}: {error}"));
    }
    fixture
}

fn pair(left: &str, right: &str) -> Vec<String> {
    vec![left.to_owned(), right.to_owned()]
}

#[test]
fn should_list_the_columnar_table_access_method_in_pg_am() {
    // Arrange
    let fixture = sql_fixture("columnar-pg-am", &[]);

    // Act
    let rows = text_rows(
        &fixture,
        "SELECT oid::TEXT, amname, amtype FROM pg_catalog.pg_am \
         WHERE amname IN ('heap', 'columnar') ORDER BY amname",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            vec!["9301".to_owned(), "columnar".to_owned(), "t".to_owned()],
            vec!["2".to_owned(), "heap".to_owned(), "t".to_owned()],
        ]
    );
}

#[test]
fn should_report_the_access_method_of_every_table_mode_in_pg_class() {
    // Arrange
    let fixture = sql_fixture("columnar-pg-class", MIXED);

    // Act
    let rows = text_rows(
        &fixture,
        "SELECT relation.relname, access_method.amname \
         FROM pg_catalog.pg_class AS relation \
         JOIN pg_catalog.pg_am AS access_method ON access_method.oid = relation.relam \
         WHERE relation.relkind = 'r' \
           AND relation.relname IN ('row_default', 'row_heap', 'col_using', 'col_with', \
                                    'col_both', 'col_ctas') \
         ORDER BY relation.relname",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            pair("col_both", "columnar"),
            pair("col_ctas", "columnar"),
            pair("col_using", "columnar"),
            pair("col_with", "columnar"),
            pair("row_default", "heap"),
            pair("row_heap", "heap"),
        ]
    );
}

#[test]
fn should_report_each_table_mode_in_pg_table_storage_within_one_database() {
    // Arrange
    let fixture = started_fixture("columnar-pg-table-storage", MIXED);

    // Act
    let rows = text_rows(
        &fixture,
        "SELECT tablename, storage_mode FROM pg_catalog.pg_table_storage \
         WHERE schemaname = 'mixed' ORDER BY tablename",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            pair("col_both", "column-store"),
            pair("col_ctas", "column-store"),
            pair("col_using", "column-store"),
            pair("col_with", "column-store"),
            pair("row_default", "row-store"),
            pair("row_heap", "row-store"),
        ]
    );
}

#[test]
fn should_keep_a_column_accelerator_distinct_from_column_authority() {
    // Arrange
    let fixture = sql_fixture(
        "columnar-accelerator-distinct",
        &[
            "CREATE TABLE accelerated (id INT, label TEXT)",
            "CREATE INDEX accelerated_label ON accelerated USING column (label)",
        ],
    );

    // Act
    let storage = text_rows(
        &fixture,
        "SELECT storage_mode FROM pg_catalog.pg_table_storage WHERE tablename = 'accelerated'",
    );
    let access_method = text_rows(
        &fixture,
        "SELECT access_method.amname FROM pg_catalog.pg_class AS relation \
         JOIN pg_catalog.pg_am AS access_method ON access_method.oid = relation.relam \
         WHERE relation.relname = 'accelerated'",
    );

    // Assert
    assert_eq!(storage, vec![vec!["column-indexed".to_owned()]]);
    assert_eq!(access_method, vec![vec!["heap".to_owned()]]);
}

#[test]
fn should_report_column_storage_in_text_explain_for_a_column_table_only() {
    // Arrange
    let fixture = sql_fixture("columnar-explain", MIXED);

    // Act
    let column = fixture
        .execute("EXPLAIN SELECT id FROM mixed.col_using WHERE label = 'column'")
        .expect("explain column table");
    let row = fixture
        .execute("EXPLAIN SELECT id FROM mixed.row_default WHERE label = 'row'")
        .expect("explain row table");

    // Assert
    assert_explain_contains(explain_plan_text(&column), "storage_mode", "column-store");
    assert_explain_contains(explain_plan_text(&row), "storage_mode", "row-store");
}

#[test]
fn should_preserve_the_column_mode_across_rename_then_schema_move() {
    // Arrange
    let fixture = started_fixture(
        "columnar-rename-move",
        &[
            "CREATE SCHEMA elsewhere",
            "CREATE TABLE movable (id INT, label TEXT) USING columnar",
            "INSERT INTO movable VALUES (1, 'kept'), (2, NULL)",
        ],
    );

    // Act
    fixture
        .execute("ALTER TABLE movable RENAME TO renamed")
        .expect("rename");
    fixture
        .execute("ALTER TABLE renamed SET SCHEMA elsewhere")
        .expect("schema move");
    let storage = text_rows(
        &fixture,
        "SELECT schemaname, tablename, storage_mode FROM pg_catalog.pg_table_storage \
         WHERE tablename = 'renamed'",
    );
    let relam = text_rows(
        &fixture,
        "SELECT relam::TEXT FROM pg_catalog.pg_class WHERE relname = 'renamed'",
    );
    let rows = text_rows(
        &fixture,
        "SELECT id::TEXT, label FROM elsewhere.renamed ORDER BY id",
    );

    // Assert
    assert_eq!(
        storage,
        vec![vec![
            "elsewhere".to_owned(),
            "renamed".to_owned(),
            "column-store".to_owned()
        ]]
    );
    assert_eq!(relam, vec![vec!["9301".to_owned()]]);
    assert_eq!(rows, vec![pair("1", "kept"), pair("2", "NULL")]);
}

#[test]
fn should_reject_conflicting_table_storage_selectors_without_publishing() {
    // Arrange
    let fixture = sql_fixture("columnar-conflicting-selectors", &[]);

    // Act
    let columnar_row =
        fixture.error("CREATE TABLE conflicted (id INT) USING columnar WITH (storage = row_store)");
    let heap_column =
        fixture.error("CREATE TABLE conflicted (id INT) USING heap WITH (storage = column_store)");
    let published = text_rows(
        &fixture,
        "SELECT count(*)::TEXT FROM pg_catalog.pg_class WHERE relname = 'conflicted'",
    );

    // Assert
    assert_eq!(columnar_row.sql_state(), "42601");
    assert_eq!(heap_column.sql_state(), "42601");
    assert_eq!(published, vec![vec!["0".to_owned()]]);
}

#[test]
fn should_reject_unknown_table_access_methods_without_publishing() {
    // Arrange
    let fixture = sql_fixture("columnar-unknown-method", &[]);

    // Act
    let create = fixture.error("CREATE TABLE unknown_method (id INT) USING brin");
    let ctas = fixture.error("CREATE TABLE unknown_method USING brin AS SELECT 1 AS id");
    let published = text_rows(
        &fixture,
        "SELECT count(*)::TEXT FROM pg_catalog.pg_class WHERE relname = 'unknown_method'",
    );

    // Assert
    assert_eq!(create.sql_state(), "0A000");
    assert_eq!(ctas.sql_state(), "0A000");
    assert_eq!(published, vec![vec!["0".to_owned()]]);
}
