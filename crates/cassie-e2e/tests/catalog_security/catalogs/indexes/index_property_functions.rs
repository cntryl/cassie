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
