//! Populated PostgreSQL type and object-identity catalog contracts for #817/#818.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_expose_postgresql_system_columns_with_declared_identities() {
    // Arrange
    let fixture = sql_fixture("issue-817-system-column-identities", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TABLE issue_817_system_column_row (id INTEGER PRIMARY KEY)")
        .expect("create a table with a physical row");
    fixture
        .execute("INSERT INTO issue_817_system_column_row VALUES (1)")
        .expect("insert the row whose system columns are inspected");
    let sql = "SELECT pg_typeof(ctid) = 'tid'::regtype, \
                      pg_typeof(xmin) = 'xid'::regtype, \
                      pg_typeof(xmax) = 'xid'::regtype, \
                      pg_typeof(cmin) = 'cid'::regtype, \
                      pg_typeof(cmax) = 'cid'::regtype, \
                      pg_typeof(tableoid) = 'oid'::regtype, \
                      tableoid = 'issue_817_system_column_row'::regclass, \
                      ctid IS NOT NULL, xmin::text <> '0', xmax::text = '0' \
               FROM issue_817_system_column_row";

    // Act
    let rows = fixture
        .execute(sql)
        .expect("read the six PostgreSQL system columns from the stored row")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

include!("../../routines/catalog/semantics.rs");

include!("../../sequences/catalog/metadata.rs");

include!("../../types/catalog/metadata.rs");

include!("../../types/catalog/semantics.rs");
