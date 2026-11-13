//! `pg_type`, `pg_enum`, `information_schema` and `regtype` presentation of enum types.
//! Authored against the PostgreSQL 18.6 oracle; not yet executed.

use crate::support_enum_types::{scalar, strings};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE SCHEMA app",
            "CREATE TYPE app.state_code AS ENUM ('open', 'closed')",
            "CREATE TYPE mood AS ENUM ('sad', 'ok')",
            "CREATE TABLE app.jobs (id int PRIMARY KEY, st app.state_code NOT NULL, history app.state_code[])",
        ],
    )
}

#[test]
fn should_publish_an_enum_pg_type_row_with_enum_semantics() {
    // Arrange
    let fixture = fixture("enum_catalog_pg_type");

    // Act
    let row = scalar(
        &fixture,
        "SELECT typtype::text || '|' || typcategory::text || '|' || typlen::text || '|' \
         || typbyval::text || '|' || typinput::text || '|' || typoutput::text || '|' \
         || typreceive::text || '|' || typsend::text \
         FROM pg_catalog.pg_type WHERE typname = 'state_code'",
    );

    // Assert
    assert_eq!(
        row.as_deref(),
        Some("e|E|4|true|enum_in|enum_out|enum_recv|enum_send")
    );
}

#[test]
fn should_publish_the_enum_in_its_own_schema_with_an_allocated_oid() {
    // Arrange
    let fixture = fixture("enum_catalog_namespace");

    // Act
    let schema = scalar(
        &fixture,
        "SELECT n.nspname FROM pg_catalog.pg_type t \
         JOIN pg_catalog.pg_namespace n ON n.oid = t.typnamespace WHERE t.typname = 'state_code'",
    );
    let allocated = scalar(
        &fixture,
        "SELECT (oid::bigint > 16383)::text FROM pg_catalog.pg_type WHERE typname = 'state_code'",
    );
    let distinct_oids = scalar(
        &fixture,
        "SELECT count(DISTINCT oid)::text FROM pg_catalog.pg_type \
         WHERE typname IN ('state_code', '_state_code', 'mood', '_mood')",
    );

    // Assert
    assert_eq!(schema.as_deref(), Some("app"));
    assert_eq!(allocated.as_deref(), Some("true"));
    assert_eq!(distinct_oids.as_deref(), Some("4"));
}

#[test]
fn should_publish_a_linked_array_row_for_each_enum() {
    // Arrange
    let fixture = fixture("enum_catalog_array_row");

    // Act
    let array_row = scalar(
        &fixture,
        "SELECT a.typtype::text || '|' || a.typcategory::text || '|' || a.typlen::text \
         FROM pg_catalog.pg_type a WHERE a.typname = '_state_code'",
    );
    let linked = scalar(
        &fixture,
        "SELECT (e.typarray = a.oid AND a.typelem = e.oid)::text \
         FROM pg_catalog.pg_type e JOIN pg_catalog.pg_type a ON a.typname = '_state_code' \
         WHERE e.typname = 'state_code'",
    );

    // Assert
    assert_eq!(array_row.as_deref(), Some("b|A|-1"));
    assert_eq!(linked.as_deref(), Some("true"));
}

#[test]
fn should_list_pg_enum_rows_in_sort_order_with_member_ids() {
    // Arrange
    let fixture = fixture("enum_catalog_pg_enum");
    fixture
        .execute("ALTER TYPE app.state_code ADD VALUE 'pending' BEFORE 'closed'")
        .unwrap();

    // Act
    let rows = strings(
        &fixture,
        "SELECT enumlabel || ':' || enumsortorder::text FROM pg_catalog.pg_enum e \
         JOIN pg_catalog.pg_type t ON t.oid = e.enumtypid WHERE t.typname = 'state_code' \
         ORDER BY enumsortorder",
    );
    let linked = scalar(
        &fixture,
        "SELECT count(*)::text FROM pg_catalog.pg_enum e \
         WHERE e.enumtypid = 'app.state_code'::regtype",
    );
    let unique_members = scalar(
        &fixture,
        "SELECT count(DISTINCT oid)::text FROM pg_catalog.pg_enum",
    );

    // Assert
    assert_eq!(rows, vec!["open:1", "pending:1.5", "closed:2"]);
    assert_eq!(linked.as_deref(), Some("3"));
    assert_eq!(
        unique_members.as_deref(),
        Some("5"),
        "3 + 2 labels, each with its own ID"
    );
}

#[test]
fn should_describe_the_pg_enum_columns() {
    // Arrange
    let fixture = fixture("enum_catalog_pg_enum_shape");

    // Act
    let result = fixture
        .execute("SELECT * FROM pg_catalog.pg_enum LIMIT 0")
        .expect("pg_enum is queryable");

    // Assert
    let names: Vec<&str> = result
        .columns()
        .iter()
        .map(|column| column.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["oid", "enumtypid", "enumsortorder", "enumlabel"]
    );
}

#[test]
fn should_report_no_pg_enum_rows_without_enum_types() {
    // Arrange
    let fixture = sql_fixture("enum_catalog_none", &[]);

    // Act
    let count = scalar(&fixture, "SELECT count(*)::text FROM pg_catalog.pg_enum");

    // Assert
    assert_eq!(count.as_deref(), Some("0"));
}

#[test]
fn should_remove_catalog_rows_when_the_enum_is_dropped() {
    // Arrange
    let fixture = fixture("enum_catalog_drop");

    // Act
    fixture.execute("DROP TYPE mood").unwrap();
    let types = scalar(
        &fixture,
        "SELECT count(*)::text FROM pg_catalog.pg_type WHERE typname IN ('mood', '_mood')",
    );
    let members = scalar(
        &fixture,
        "SELECT count(*)::text FROM pg_catalog.pg_enum WHERE enumlabel IN ('sad', 'ok')",
    );

    // Assert
    assert_eq!(types.as_deref(), Some("0"));
    assert_eq!(members.as_deref(), Some("0"));
}

#[test]
fn should_report_enum_columns_as_user_defined_in_information_schema() {
    // Arrange
    let fixture = fixture("enum_catalog_information_schema");

    // Act
    let columns = strings(
        &fixture,
        "SELECT column_name || '|' || data_type || '|' || udt_schema || '|' || udt_name \
         FROM information_schema.columns \
         WHERE table_schema = 'app' AND table_name = 'jobs' ORDER BY ordinal_position",
    );

    // Assert
    assert_eq!(
        columns,
        vec![
            "id|integer|pg_catalog|int4",
            "st|USER-DEFINED|app|state_code",
            "history|ARRAY|pg_catalog|_state_code",
        ]
    );
}

#[test]
fn should_link_pg_attribute_to_the_enum_and_array_types() {
    // Arrange
    let fixture = fixture("enum_catalog_pg_attribute");

    // Act
    let types = strings(
        &fixture,
        "SELECT a.attname || '|' || t.typname FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_type t ON t.oid = a.atttypid \
         WHERE c.relname = 'jobs' AND a.attnum > 0 ORDER BY a.attnum",
    );

    // Assert
    assert_eq!(
        types,
        vec!["id|int4", "st|state_code", "history|_state_code"]
    );
}

#[test]
fn should_resolve_enum_names_through_regtype_and_pg_typeof() {
    // Arrange
    let fixture = fixture("enum_catalog_regtype");

    // Act
    let by_name = scalar(&fixture, "SELECT 'mood'::regtype::text");
    let array_by_name = scalar(&fixture, "SELECT 'mood[]'::regtype::text");
    let same_oid = scalar(
        &fixture,
        "SELECT ('mood'::regtype::oid = (SELECT oid FROM pg_catalog.pg_type WHERE typname = 'mood'))::text",
    );
    let typeof_scalar = scalar(&fixture, "SELECT pg_typeof('ok'::mood)::text");
    let typeof_array = scalar(&fixture, "SELECT pg_typeof(ARRAY['ok']::mood[])::text");
    let qualified = scalar(&fixture, "SELECT 'app.state_code'::regtype::oid > 16383");
    let missing = fixture.error("SELECT 'no_such_enum'::regtype");

    // Assert
    assert_eq!(by_name.as_deref(), Some("mood"));
    assert_eq!(array_by_name.as_deref(), Some("mood[]"));
    assert_eq!(same_oid.as_deref(), Some("true"));
    assert_eq!(typeof_scalar.as_deref(), Some("mood"));
    assert_eq!(typeof_array.as_deref(), Some("mood[]"));
    assert_eq!(qualified.as_deref(), Some("true"));
    assert_eq!(missing.sql_state(), "42704");
}

#[test]
fn should_reflect_a_renamed_value_and_type_in_the_catalogs() {
    // Arrange
    let fixture = fixture("enum_catalog_after_rename");

    // Act
    fixture
        .execute("ALTER TYPE mood RENAME VALUE 'ok' TO 'fine'")
        .unwrap();
    fixture
        .execute("ALTER TYPE mood RENAME TO feeling")
        .unwrap();
    let labels = strings(
        &fixture,
        "SELECT enumlabel FROM pg_catalog.pg_enum \
         WHERE enumtypid = 'feeling'::regtype ORDER BY enumsortorder",
    );
    let names = strings(
        &fixture,
        "SELECT typname FROM pg_catalog.pg_type WHERE typname IN ('mood', 'feeling', '_feeling') \
         ORDER BY typname",
    );

    // Assert
    assert_eq!(labels, vec!["sad", "fine"]);
    assert_eq!(names, vec!["_feeling", "feeling"]);
}
