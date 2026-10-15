//! PostgreSQL 18.6 `information_schema` descriptor and row contracts for #817.

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

include!("../identity_privileges/information_schema.rs");

include!("../routines/information_schema.rs");

include!("../triggers/information_schema.rs");

include!("../types/information_schema.rs");
