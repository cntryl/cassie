//! PostgreSQL object-description and object-address lookup contracts (#817).

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_describe_live_objects_given_supported_catalog_objects_when_pg_describe_object_is_called()
{
    // Arrange
    let fixture = sql_fixture("issue-817-describe-live-table", &[]);
    fixture
        .execute("CREATE TABLE issue_817_described_table (id INTEGER)")
        .expect("create a live relation for the catalog lookup");
    fixture.cassie.startup().expect("start catalog fixture");

    // Act
    let result = fixture.execute(
        "SELECT pg_catalog.pg_describe_object(\
             'pg_catalog.pg_class'::regclass, \
             'issue_817_described_table'::regclass, 0)",
    );

    // Assert
    let result = result.expect("describe the table by its catalog object address");
    assert_eq!(result.columns()[0].type_oid, 25);
    assert_eq!(
        result.rows(),
        &[vec![Value::String(
            "table issue_817_described_table".to_owned()
        )]]
    );
}

#[test]
fn should_roundtrip_object_addresses_given_live_objects_when_address_helpers_are_called() {
    // Arrange
    let fixture = sql_fixture("issue-817-object-address-roundtrip", &[]);
    fixture
        .execute("CREATE TABLE issue_817_address_target (id INTEGER)")
        .expect("create a live relation for the object-address round trip");
    fixture.cassie.startup().expect("start catalog fixture");

    // Act
    let result = fixture.execute(
        "SELECT identified.type = 'table', \
                identified.object_names = ARRAY['issue_817_address_target']::TEXT[], \
                identified.object_args = ARRAY[]::TEXT[], \
                resolved.classid = 'pg_catalog.pg_class'::regclass, \
                resolved.objid = 'issue_817_address_target'::regclass, \
                resolved.objsubid = 0 \
         FROM pg_catalog.pg_identify_object_as_address(\
                  'pg_catalog.pg_class'::regclass, \
                  'issue_817_address_target'::regclass, 0) AS identified \
         CROSS JOIN LATERAL pg_catalog.pg_get_object_address(\
             identified.type, identified.object_names, identified.object_args) AS resolved",
    );

    // Assert
    let result = result.expect("identify and resolve the same relation address");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [16; 6]
    );
    assert_eq!(
        result.rows(),
        &[vec![Value::Bool(true); 6]],
        "PostgreSQL 18.6 preserves class OID, object identity, and subobject identity"
    );
}
