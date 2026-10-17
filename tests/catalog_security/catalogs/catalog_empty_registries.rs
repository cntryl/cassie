//! Native catalog column shapes and empty registries for absent object families.
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_report_no_pg_auth_members_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_auth_members", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_auth_members");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_db_role_setting_objects_given_a_fresh_catalog_when_the_registry_is_queried()
{
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_db_role_setting", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_db_role_setting");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_default_acl_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_default_acl", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_default_acl");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_foreign_data_wrapper_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_foreign_data_wrapper", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_foreign_data_wrapper");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_foreign_server_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_foreign_server", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_foreign_server");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_foreign_table_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_foreign_table", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_foreign_table");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_init_privs_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_init_privs", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_init_privs");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_largeobject_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_largeobject", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_largeobject");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_largeobject_metadata_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_largeobject_metadata", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_largeobject_metadata");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_publication_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_publication", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_publication");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_publication_namespace_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_publication_namespace", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_publication_namespace");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_publication_rel_objects_given_a_fresh_catalog_when_the_registry_is_queried()
{
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_publication_rel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_publication_rel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_replication_origin_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_replication_origin", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_replication_origin");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_seclabel_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_seclabel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_seclabel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_shdescription_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_shdescription", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_shdescription");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_shseclabel_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_shseclabel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_shseclabel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_subscription_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_subscription", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_subscription");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_subscription_rel_objects_given_a_fresh_catalog_when_the_registry_is_queried()
{
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_subscription_rel", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_subscription_rel");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_transform_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_transform", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_transform");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_user_mapping_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_user_mapping", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_user_mapping");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_report_no_pg_enum_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_enum", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_enum");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_enum_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_enum", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_enum LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["oid", "enumtypid", "enumsortorder", "enumlabel"]
    );
}

#[test]
fn should_report_no_pg_partitioned_table_objects_given_a_fresh_catalog_when_the_registry_is_queried(
) {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_partitioned_table", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_partitioned_table");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_partitioned_table_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_partitioned_table", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_partitioned_table LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "partrelid",
            "partstrat",
            "partnatts",
            "partdefid",
            "partattrs",
            "partclass",
            "partcollation",
            "partexprs"
        ]
    );
}

#[test]
fn should_report_no_pg_trigger_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_trigger");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_trigger_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_trigger LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "oid",
            "tgrelid",
            "tgparentid",
            "tgname",
            "tgfoid",
            "tgtype",
            "tgenabled",
            "tgisinternal",
            "tgconstrrelid",
            "tgconstrindid",
            "tgconstraint",
            "tgdeferrable",
            "tginitdeferred",
            "tgnargs",
            "tgattr",
            "tgargs",
            "tgqual",
            "tgoldtable",
            "tgnewtable"
        ]
    );
}

#[test]
fn should_report_no_pg_policy_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_policy", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_policy");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_policy_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_policy", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_policy LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "oid",
            "polname",
            "polrelid",
            "polcmd",
            "polpermissive",
            "polroles",
            "polqual",
            "polwithcheck"
        ]
    );
}

#[test]
fn should_report_no_pg_event_trigger_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_event_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_event_trigger");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_event_trigger_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_event_trigger", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_event_trigger LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "oid",
            "evtname",
            "evtevent",
            "evtowner",
            "evtfoid",
            "evtenabled",
            "evttags"
        ]
    );
}

#[test]
fn should_report_no_pg_parameter_acl_objects_given_a_fresh_catalog_when_the_registry_is_queried() {
    // Arrange
    let fixture = sql_fixture("catalog209-empty-pg_parameter_acl", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM pg_catalog.pg_parameter_acl");
    // Assert
    assert_eq!(
        result.expect("empty registry").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_describe_pg_parameter_acl_given_a_fresh_catalog_when_no_objects_exist() {
    // Arrange
    let fixture = sql_fixture("catalog209-shape-pg_parameter_acl", &[]);
    // Act
    let result = fixture.execute("SELECT * FROM pg_catalog.pg_parameter_acl LIMIT 0");
    // Assert
    let result = result.expect("catalog shape");
    assert_eq!(result.rows(), &[] as &[Vec<Value>]);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["oid", "parname", "paracl"]
    );
}
