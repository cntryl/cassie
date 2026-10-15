//! PostgreSQL 18.6 index introspection functions and identities for #817.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn btree_index_fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE TABLE issue_817_index_properties (id INT PRIMARY KEY, value TEXT)",
            "CREATE INDEX issue_817_index_properties_idx ON issue_817_index_properties(value)",
        ],
    );
    fixture.cassie.startup().expect("startup");
    fixture
}

#[test]
fn should_report_access_method_oid_for_a_created_index() {
    // Arrange
    let fixture = btree_index_fixture("issue-817-index-access-method-oid");

    // Act
    let selected = fixture.execute(
        "SELECT index_class.relname, index_class.relam \
         FROM pg_catalog.pg_class AS index_class \
         WHERE index_class.relname = 'issue_817_index_properties_idx'",
    );

    // Assert
    let selected = selected.expect("read the access method identity for a created index");
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 26]
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("issue_817_index_properties_idx".to_owned()),
            Value::Int64(403),
        ]]
    );
}

#[test]
fn should_report_column_property_matrix_for_a_created_btree_index() {
    // Arrange
    let fixture = btree_index_fixture("issue-817-index-column-properties");

    // Act
    let properties = fixture.execute(
        "SELECT pg_catalog.pg_index_column_has_property(index_class.oid, 1, 'orderable'), \
                pg_catalog.pg_index_column_has_property(index_class.oid, 1, 'returnable'), \
                pg_catalog.pg_index_column_has_property(index_class.oid, 1, 'search_array') \
         FROM pg_catalog.pg_class AS index_class \
         WHERE index_class.relname = 'issue_817_index_properties_idx'",
    );

    // Assert
    let properties = properties.expect("read column properties for the created btree index");
    assert_eq!(
        properties
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 3]
    );
    assert_eq!(
        properties.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_report_index_scan_property_matrix_for_a_created_btree_index() {
    // Arrange
    let fixture = btree_index_fixture("issue-817-index-scan-properties");

    // Act
    let properties = fixture.execute(
        "SELECT pg_catalog.pg_index_has_property(index_class.oid, 'clusterable'), \
                pg_catalog.pg_index_has_property(index_class.oid, 'index_scan') \
         FROM pg_catalog.pg_class AS index_class \
         WHERE index_class.relname = 'issue_817_index_properties_idx'",
    );

    // Assert
    let properties = properties.expect("read scan properties for the created btree index");
    assert_eq!(
        properties
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 2]
    );
    assert_eq!(
        properties.rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_report_access_method_property_matrix_for_btree() {
    // Arrange
    let fixture = sql_fixture("issue-817-btree-access-method-properties", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let properties = fixture.execute(
        "SELECT pg_catalog.pg_indexam_has_property(403, 'can_order'), \
                pg_catalog.pg_indexam_has_property(403, 'can_unique'), \
                pg_catalog.pg_indexam_has_property(403, 'can_multi_col'), \
                pg_catalog.pg_indexam_has_property(403, 'can_exclude'), \
                pg_catalog.pg_indexam_has_property(403, 'can_include'), \
                pg_catalog.pg_indexam_has_property(403, 'not_a_property') IS NULL",
    );

    // Assert
    let properties = properties.expect("read access method properties for the created btree index");
    assert_eq!(
        properties
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 6]
    );
    assert_eq!(properties.rows(), &[vec![Value::Bool(true); 6]]);
}
