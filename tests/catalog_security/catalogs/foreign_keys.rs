//! PostgreSQL catalog foreign-key metadata.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_report_catalog_foreign_keys_given_builtin_catalogs_when_pg_get_catalog_foreign_keys_is_called(
) {
    // Arrange
    let fixture = sql_fixture("issue-817-catalog-foreign-keys", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture
        .execute(
            "SELECT fkcols::text, pktable::regclass::text, pkcols::text, is_array, is_opt \
             FROM pg_catalog.pg_get_catalog_foreign_keys() \
             WHERE fktable = 'pg_catalog.pg_proc'::regclass \
               AND fkcols::text = '{pronamespace}'",
        )
        .expect("read a declared pg_proc to pg_namespace catalog relationship");

    // Assert
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 25, 25, 16, 16]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("{pronamespace}".to_owned()),
            Value::String("pg_namespace".to_owned()),
            Value::String("{oid}".to_owned()),
            Value::Bool(false),
            Value::Bool(false),
        ]]
    );
}
