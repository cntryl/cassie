//! `pg_type`, `pg_class`, `pg_attribute` and `regtype` publication for composite types.

use crate::support_composite_types::{first_column_text, only_text};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, &["CREATE TYPE address AS (street text, zip int)"])
}

fn count(fixture: &SqlFixture, sql: &str) -> i64 {
    match fixture.rows(sql).as_slice() {
        [row] => match row.as_slice() {
            [Value::Int64(count)] => *count,
            _ => panic!("expected a count cell"),
        },
        _ => panic!("expected one row"),
    }
}

#[test]
fn should_publish_a_pg_type_row_for_the_composite_type() {
    // Arrange
    let fixture = fixture("composite_pg_type");

    // Act
    let rows = fixture.rows(
        "SELECT typtype, typcategory, typnotnull, typtypmod, typlen FROM pg_type \
         WHERE typname = 'address'",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("c".into()),
            Value::String("C".into()),
            Value::Bool(false),
            Value::Int64(-1),
            Value::Int64(-1),
        ]]
    );
}

#[test]
fn should_link_pg_type_to_a_relkind_c_pg_class_row() {
    // Arrange
    let fixture = fixture("composite_pg_class");

    // Act
    let linked = count(
        &fixture,
        "SELECT count(*) FROM pg_type t JOIN pg_class c ON c.oid = t.typrelid \
         WHERE t.typname = 'address' AND c.relname = 'address' AND c.relkind = 'c' \
         AND c.reltype = t.oid",
    );

    // Assert
    assert_eq!(linked, 1);
}

#[test]
fn should_publish_the_array_type_with_the_element_link() {
    // Arrange
    let fixture = fixture("composite_array_type");

    // Act
    let linked = count(
        &fixture,
        "SELECT count(*) FROM pg_type t JOIN pg_type a ON a.oid = t.typarray \
         WHERE t.typname = 'address' AND a.typname = '_address' \
         AND a.typelem = t.oid AND a.typcategory = 'A'",
    );

    // Assert
    assert_eq!(linked, 1);
}

#[test]
fn should_publish_pg_attribute_rows_for_each_field_in_order() {
    // Arrange
    let fixture = fixture("composite_pg_attribute");

    // Act
    let names = first_column_text(
        &fixture
            .rows(concat!("SELECT a.attname ", "FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid_oid WHERE c.relname = 'address' AND c.relkind = 'c' ORDER BY a.attnum")),
    );
    let types = fixture
        .rows(concat!("SELECT a.atttypid ", "FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid_oid WHERE c.relname = 'address' AND c.relkind = 'c' ORDER BY a.attnum"));

    // Assert
    assert_eq!(
        names,
        vec![Some("street".to_owned()), Some("zip".to_owned())]
    );
    assert_eq!(types, vec![vec![Value::Int64(25)], vec![Value::Int64(23)]]);
}

#[test]
fn should_resolve_user_composite_types_through_regtype() {
    // Arrange
    let fixture = fixture("composite_regtype");

    // Act
    let cast = only_text(&fixture.rows("SELECT 'address'::regtype::text"));
    let array = only_text(&fixture.rows("SELECT 'address[]'::regtype::text"));
    let same_oid = count(
        &fixture,
        "SELECT count(*) FROM pg_type WHERE typname = 'address' \
         AND oid = to_regtype('address')::oid",
    );
    let missing = only_text(&fixture.rows("SELECT to_regtype('nope')::text"));

    // Assert
    assert_eq!(cast, Some("address".to_owned()));
    assert_eq!(array, Some("address[]".to_owned()));
    assert_eq!(same_oid, 1);
    assert_eq!(missing, None);
}

#[test]
fn should_report_the_composite_type_name_from_pg_typeof() {
    // Arrange
    let fixture = fixture("composite_pg_typeof");

    // Act
    let named = only_text(&fixture.rows("SELECT pg_typeof('(a,1)'::address)::text"));
    let anonymous = only_text(&fixture.rows("SELECT pg_typeof(ROW(1, 2))::text"));

    // Assert
    assert_eq!(named, Some("address".to_owned()));
    assert_eq!(anonymous, Some("record".to_owned()));
}

#[test]
fn should_remove_catalog_rows_when_the_type_is_dropped() {
    // Arrange
    let fixture = fixture("composite_catalog_drop");
    fixture.execute("DROP TYPE address").unwrap();

    // Act
    let types = count(
        &fixture,
        "SELECT count(*) FROM pg_type WHERE typname IN ('address', '_address')",
    );
    let classes = count(
        &fixture,
        "SELECT count(*) FROM pg_class WHERE relname = 'address'",
    );
    let attributes = count(
        &fixture,
        concat!("SELECT count(*) ", "FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid_oid WHERE c.relname = 'address' AND c.relkind = 'c'"),
    );
    let resolved = only_text(&fixture.rows("SELECT to_regtype('address')::text"));

    // Assert
    assert_eq!((types, classes, attributes), (0, 0, 0));
    assert_eq!(resolved, None);
}

#[test]
fn should_resolve_regclass_to_the_composite_relkind_c_row() {
    // Arrange
    let fixture = fixture("composite_regclass");

    // Act
    let same_oid = count(
        &fixture,
        "SELECT count(*) FROM pg_type WHERE typname = 'address' \
         AND typrelid = 'address'::regclass::oid",
    );
    let label = only_text(&fixture.rows("SELECT 'address'::regclass::text"));
    fixture
        .execute("DROP TYPE address")
        .expect("drop type should succeed");
    let dropped = only_text(&fixture.rows("SELECT to_regclass('address')::text"));

    // Assert
    assert_eq!(same_oid, 1);
    assert_eq!(label, Some("address".to_owned()));
    assert_eq!(dropped, None);
}
