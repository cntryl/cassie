//! PostgreSQL 18.6 information_schema descriptor and row contracts for #817.

use crate::support_sql_fixture::sql_fixture;
use cassie::executor::QueryResult;
use cassie::types::Value;

const PG18_SQL_IDENTIFIER_OID: i64 = 13_718;
const PG18_CARDINAL_NUMBER_OID: i64 = 13_713;
const PG18_CHARACTER_DATA_OID: i64 = 13_716;
const PG18_TIME_STAMP_OID: i64 = 13_724;
const PG18_YES_OR_NO_OID: i64 = 13_726;

fn assert_descriptor(result: &QueryResult, expected: &[(&str, i64, i16, i32, bool)]) {
    let actual = result
        .columns()
        .iter()
        .map(|column| {
            (
                column.name.clone(),
                column.type_oid,
                column.typlen,
                column.atttypmod,
                column.nullable,
            )
        })
        .collect::<Vec<_>>();
    let expected = expected
        .iter()
        .map(|(name, type_oid, typlen, atttypmod, nullable)| {
            (
                (*name).to_owned(),
                *type_oid,
                *typlen,
                *atttypmod,
                *nullable,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn should_report_builtin_parameter_metadata_in_information_schema() {
    // Arrange
    let fixture = sql_fixture("issue-817-information-schema-parameters", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT specific_catalog, specific_schema, specific_name, ordinal_position, \
                    parameter_mode, is_result, parameter_name, data_type, udt_name, \
                    parameter_default \
             FROM information_schema.parameters \
             WHERE specific_schema = 'pg_catalog' \
               AND specific_name LIKE 'pg_sleep_for%' \
             ORDER BY ordinal_position",
        )
        .expect("read the built-in pg_sleep_for parameter metadata");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("specific_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("specific_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("specific_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("ordinal_position", PG18_CARDINAL_NUMBER_OID, 4, -1, true),
            ("parameter_mode", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("is_result", PG18_YES_OR_NO_OID, -1, -1, true),
            ("parameter_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("data_type", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("udt_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("parameter_default", PG18_CHARACTER_DATA_OID, -1, -1, true),
        ],
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("postgres".to_owned()),
            Value::String("pg_catalog".to_owned()),
            Value::String("pg_sleep_for_3935".to_owned()),
            Value::Int64(1),
            Value::String("IN".to_owned()),
            Value::String("NO".to_owned()),
            Value::String(String::new()),
            Value::String("interval".to_owned()),
            Value::String("interval".to_owned()),
            Value::Null,
        ]]
    );
}

#[test]
fn should_report_array_element_metadata_in_information_schema() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-element-types",
        &["CREATE TABLE issue_817_element_types (id INTEGER, labels INTEGER[])"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT object_catalog, object_schema, object_name, object_type, \
                    collection_type_identifier, data_type, udt_catalog, udt_schema, \
                    udt_name, dtd_identifier \
             FROM information_schema.element_types \
             WHERE object_catalog = current_database() \
               AND object_schema = 'public' \
               AND object_name = 'issue_817_element_types'",
        )
        .expect("read the declared array element metadata");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("object_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("object_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("object_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("object_type", PG18_CHARACTER_DATA_OID, -1, -1, true),
            (
                "collection_type_identifier",
                PG18_SQL_IDENTIFIER_OID,
                64,
                -1,
                true,
            ),
            ("data_type", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("udt_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("udt_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("udt_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("dtd_identifier", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
        ],
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("postgres".to_owned()),
            Value::String("public".to_owned()),
            Value::String("issue_817_element_types".to_owned()),
            Value::String("TABLE".to_owned()),
            Value::String("2".to_owned()),
            Value::String("integer".to_owned()),
            Value::String("postgres".to_owned()),
            Value::String("pg_catalog".to_owned()),
            Value::String("int4".to_owned()),
            Value::String("a2".to_owned()),
        ]]
    );
}

#[test]
fn should_describe_trigger_metadata_without_trigger_objects() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-triggers-empty",
        &["CREATE TABLE issue_817_trigger_metadata (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT trigger_catalog, trigger_schema, trigger_name, event_manipulation, \
                    event_object_catalog, event_object_schema, event_object_table, \
                    action_orientation, action_timing, created \
             FROM information_schema.triggers \
             WHERE event_object_schema = 'public' \
               AND event_object_table = 'issue_817_trigger_metadata' \
             ORDER BY trigger_name, event_manipulation",
        )
        .expect("describe trigger metadata for a table with no triggers");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("trigger_catalog", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("trigger_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("trigger_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("event_manipulation", PG18_CHARACTER_DATA_OID, -1, -1, true),
            (
                "event_object_catalog",
                PG18_SQL_IDENTIFIER_OID,
                64,
                -1,
                true,
            ),
            ("event_object_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("event_object_table", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("action_orientation", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("action_timing", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("created", PG18_TIME_STAMP_OID, 8, -1, true),
        ],
    );
    assert!(selected.rows().is_empty());
}

fn assert_owner_privileges_in_view(view_name: &str, fixture_label: &str) {
    let fixture = sql_fixture(
        fixture_label,
        &["CREATE TABLE issue_817_table_privileges (id INTEGER)"],
    );
    fixture.cassie.startup().expect("startup");

    let descriptor = fixture
        .execute(&format!(
            "SELECT grantee, table_schema, table_name, privilege_type, \
                    is_grantable, with_hierarchy \
             FROM information_schema.{view_name} WHERE false"
        ))
        .unwrap_or_else(|error| panic!("describe information_schema.{view_name}: {error}"));
    assert_descriptor(
        &descriptor,
        &[
            ("grantee", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("table_schema", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("table_name", PG18_SQL_IDENTIFIER_OID, 64, -1, true),
            ("privilege_type", PG18_CHARACTER_DATA_OID, -1, -1, true),
            ("is_grantable", PG18_YES_OR_NO_OID, -1, -1, true),
            ("with_hierarchy", PG18_YES_OR_NO_OID, -1, -1, true),
        ],
    );

    let selected = fixture
        .execute(&format!(
            "SELECT grantee = current_user, table_schema, table_name, privilege_type, \
                    is_grantable, with_hierarchy \
             FROM information_schema.{view_name} \
             WHERE table_schema = 'public' \
               AND table_name = 'issue_817_table_privileges' \
             ORDER BY privilege_type"
        ))
        .unwrap_or_else(|error| panic!("read information_schema.{view_name} rows: {error}"));

    let expected = [
        ("DELETE", "NO"),
        ("INSERT", "NO"),
        ("REFERENCES", "NO"),
        ("SELECT", "YES"),
        ("TRIGGER", "NO"),
        ("TRUNCATE", "NO"),
        ("UPDATE", "NO"),
    ]
    .into_iter()
    .map(|(privilege, with_hierarchy)| {
        vec![
            Value::Bool(true),
            Value::String("public".to_owned()),
            Value::String("issue_817_table_privileges".to_owned()),
            Value::String(privilege.to_owned()),
            Value::String("YES".to_owned()),
            Value::String(with_hierarchy.to_owned()),
        ]
    })
    .collect::<Vec<_>>();
    assert_eq!(selected.rows(), expected.as_slice());
}

#[test]
fn should_report_owner_privileges_in_information_schema_role_table_grants() {
    // Arrange
    // The created relation is owned by the current test session; no extra role or grant is needed.

    // Act
    assert_owner_privileges_in_view(
        "role_table_grants",
        "issue-817-information-schema-role-table-grants",
    );

    // Assert
    // The helper asserts the relation owner, seven privileges, and their descriptors.
}

#[test]
fn should_report_owner_privileges_in_information_schema_table_privileges() {
    // Arrange
    // The created relation is owned by the current test session; no extra role or grant is needed.

    // Act
    assert_owner_privileges_in_view(
        "table_privileges",
        "issue-817-information-schema-table-privileges",
    );

    // Assert
    // The helper asserts the relation owner, seven privileges, and their descriptors.
}

#[test]
fn should_describe_pg_constraint_catalog_columns_with_postgresql_types() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-constraint-descriptor", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT oid, conname, contype, conenforced, convalidated, conrelid \
             FROM pg_catalog.pg_constraint LIMIT 0",
        )
        .expect("describe PostgreSQL 18 constraint catalog fields");

    // Assert
    assert_descriptor(
        &selected,
        &[
            ("oid", 26, 4, -1, false),
            ("conname", 19, 64, -1, false),
            ("contype", 18, 1, -1, false),
            ("conenforced", 16, 1, -1, false),
            ("convalidated", 16, 1, -1, false),
            ("conrelid", 26, 4, -1, false),
        ],
    );
}

#[test]
fn should_render_created_check_constraint_definition() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-check-constraint-definition",
        &["CREATE TABLE issue_817_constraint_definition (\
            amount INTEGER, \
            CONSTRAINT issue_817_amount_positive CHECK (amount > 0))"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_get_constraintdef(oid) AS definition \
             FROM pg_catalog.pg_constraint \
             WHERE conrelid = 'issue_817_constraint_definition'::regclass \
               AND conname = 'issue_817_amount_positive'",
        )
        .expect("render the created check constraint definition");

    // Assert
    assert_descriptor(&selected, &[("definition", 25, -1, -1, true)]);
    assert_eq!(
        selected.rows(),
        &[vec![Value::String("CHECK ((amount > 0))".to_owned())]]
    );
}
