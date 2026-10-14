use super::*;

#[test]
fn should_report_visibility_for_each_builtin_object_family() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-object-visibility-functions",
        &[
            "CREATE TABLE issue_817_visibility_objects (kind TEXT, object_oid INTEGER)",
            "INSERT INTO issue_817_visibility_objects (kind, object_oid) VALUES \
             ('type', 23), ('function', 2108), ('operator', 96), ('opclass', 1978), \
             ('opfamily', 1976), ('tsconfig', 3748), ('tsdict', 3765), \
             ('tsparser', 3722), ('tstemplate', 3727)",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let cases = [
        (
            "SELECT pg_catalog.pg_type_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'type'",
            "PostgreSQL reports the built-in int4 type as visible",
        ),
        (
            "SELECT pg_catalog.pg_function_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'function'",
            "PostgreSQL reports the built-in integer sum overload as visible",
        ),
        (
            "SELECT pg_catalog.pg_operator_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'operator'",
            "PostgreSQL reports the built-in integer equality operator as visible",
        ),
        (
            "SELECT pg_catalog.pg_opclass_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'opclass'",
            "PostgreSQL reports the built-in int4 B-tree operator class as visible",
        ),
        (
            "SELECT pg_catalog.pg_opfamily_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'opfamily'",
            "PostgreSQL reports the built-in integer B-tree operator family as visible",
        ),
        (
            "SELECT pg_catalog.pg_ts_config_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'tsconfig'",
            "PostgreSQL reports the built-in simple text-search configuration as visible",
        ),
        (
            "SELECT pg_catalog.pg_ts_dict_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'tsdict'",
            "PostgreSQL reports the built-in simple text-search dictionary as visible",
        ),
        (
            "SELECT pg_catalog.pg_ts_parser_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'tsparser'",
            "PostgreSQL reports the built-in default text-search parser as visible",
        ),
        (
            "SELECT pg_catalog.pg_ts_template_is_visible(object_oid) \
             FROM issue_817_visibility_objects WHERE kind = 'tstemplate'",
            "PostgreSQL reports the built-in simple text-search template as visible",
        ),
    ];

    // Act
    let mut mismatches = Vec::new();
    for (sql, description) in cases {
        match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![Value::Bool(true)]] => {}
            Ok(result) => mismatches.push(format!(
                "{description}: expected [[true]], got {:?}",
                result.rows()
            )),
            Err(error) => mismatches.push(format!("{description}: {error}")),
        }
    }

    // Assert
    assert!(
        mismatches.is_empty(),
        "PostgreSQL 18.6 built-in visibility results differ:\n{}",
        mismatches.join("\n")
    );
}
