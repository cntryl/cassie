use super::*;

#[test]
fn should_report_visibility_for_each_builtin_object_family() {
    // Arrange
    // These object OIDs are the PostgreSQL 18.6 built-ins used by the oracle.
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

#[test]
fn should_resolve_shadowed_type_names_by_search_path_precedence() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-shadowed-regtype-visibility",
        &[
            "CREATE SCHEMA issue_818_shadow_a",
            "CREATE SCHEMA issue_818_shadow_b",
            "CREATE TYPE issue_818_shadow_a.shadow_kind AS ENUM ('a')",
            "CREATE TYPE issue_818_shadow_b.shadow_kind AS ENUM ('b')",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let query = "SELECT n.nspname, pg_catalog.pg_type_is_visible(t.oid), \
                pg_catalog.to_regtype('shadow_kind')::oid = t.oid \
         FROM pg_catalog.pg_type AS t \
         JOIN pg_catalog.pg_namespace AS n ON n.oid = t.typnamespace \
         WHERE t.typname = 'shadow_kind' \
           AND n.nspname IN ('issue_818_shadow_a', 'issue_818_shadow_b') \
         ORDER BY n.nspname";
    fixture
        .execute("SET search_path TO issue_818_shadow_a, issue_818_shadow_b, public")
        .expect("put the first type schema first");

    // Act
    let first_path = fixture
        .execute(query)
        .expect("inspect type visibility in the first search path");
    fixture
        .execute("SET search_path TO issue_818_shadow_b, issue_818_shadow_a, public")
        .expect("put the second type schema first");
    let second_path = fixture
        .execute(query)
        .expect("inspect type visibility in the reversed search path");

    // Assert
    assert_eq!(
        first_path.rows(),
        vec![
            vec![
                Value::String("issue_818_shadow_a".to_owned()),
                Value::Bool(true),
                Value::Bool(true),
            ],
            vec![
                Value::String("issue_818_shadow_b".to_owned()),
                Value::Bool(false),
                Value::Bool(false),
            ],
        ]
    );
    assert_eq!(
        second_path.rows(),
        vec![
            vec![
                Value::String("issue_818_shadow_a".to_owned()),
                Value::Bool(false),
                Value::Bool(false),
            ],
            vec![
                Value::String("issue_818_shadow_b".to_owned()),
                Value::Bool(true),
                Value::Bool(true),
            ],
        ]
    );
}

#[test]
fn should_apply_schema_usage_to_regtype_visibility() {
    // Arrange
    let fixture = sql_fixture(
        "issue-818-regtype-schema-usage-visibility",
        &[
            "CREATE ROLE issue_818_hidden_reader",
            "CREATE SCHEMA issue_818_hidden",
            "CREATE TYPE issue_818_hidden.hidden_kind AS ENUM ('hidden')",
            "REVOKE USAGE ON SCHEMA issue_818_hidden FROM PUBLIC",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let reader = fixture
        .cassie
        .create_session("issue_818_hidden_reader", None);
    fixture
        .cassie
        .execute_sql(
            &reader,
            "SET search_path TO issue_818_hidden, public",
            vec![],
        )
        .expect("set restricted role search path");

    // Act
    let selected = fixture
        .cassie
        .execute_sql(
            &reader,
            "SELECT pg_catalog.pg_type_is_visible(t.oid), \
                    pg_catalog.to_regtype('hidden_kind') IS NULL \
             FROM pg_catalog.pg_type AS t \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = t.typnamespace \
             WHERE n.nspname = 'issue_818_hidden' AND t.typname = 'hidden_kind'",
            vec![],
        )
        .expect("inspect invisible type by catalog OID");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Bool(false), Value::Bool(true)]]
    );
}
