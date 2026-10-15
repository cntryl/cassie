//! PostgreSQL 18 base type introspection for built-in scalar and array types.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_report_pg_basetype_oids_for_builtin_types() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-basetype-result-identity", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture.execute("SELECT pg_catalog.pg_basetype(23), pg_catalog.pg_basetype(1007)");

    // Assert
    let result = result.expect("read PostgreSQL base type identities");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2_206, 2_206]
    );
    assert_eq!(
        result.rows(),
        &[vec![Value::Int64(23), Value::Int64(1_007),]]
    );
}
