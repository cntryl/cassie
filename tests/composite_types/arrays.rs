//! Arrays of named composite types: text I/O, storage, attributes and catalogs (#1268).

use crate::support_composite_types::{first_column_text, only_text, LocalStore};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::Cassie;

const SETUP: [&str; 3] = [
    "CREATE TYPE address AS (street text, zip int)",
    "CREATE TYPE route AS (label text, stops address[])",
    "CREATE TABLE trips (id int, stops address[], plan route)",
];

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(label, &SETUP)
}

fn text(fixture: &SqlFixture, sql: &str) -> Option<String> {
    only_text(&fixture.rows(sql))
}

fn run(cassie: &Cassie, sql: &str) -> Vec<Vec<cassie::types::Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, vec![])
        .unwrap_or_else(|_| panic!("statement failed: {sql}"))
        .rows()
        .to_vec()
}

#[test]
fn should_render_an_array_of_records_with_nested_quoting() {
    // Arrange
    let fixture = fixture("composite_array_text_out");

    // Act
    let rendered = text(
        &fixture,
        "SELECT ARRAY[ROW('a b', 1)::address, NULL, ROW('c', 2)::address]::text",
    );

    // Assert
    assert_eq!(rendered.as_deref(), Some(r#"{"(\"a b\",1)",NULL,"(c,2)"}"#));
}

#[test]
fn should_parse_array_text_into_typed_records() {
    // Arrange
    let fixture = fixture("composite_array_text_in");

    // Act
    let round_trip = text(
        &fixture,
        r#"SELECT '{"(\"a b\",1)",NULL,"(c,2)"}'::address[]::text"#,
    );
    let field = text(
        &fixture,
        r#"SELECT (('{"(\"quote \\\"d\\\"\",7)"}'::address[])[1]).street"#,
    );
    let empty = text(&fixture, "SELECT '{}'::address[]::text");

    // Assert
    assert_eq!(
        round_trip.as_deref(),
        Some(r#"{"(\"a b\",1)",NULL,"(c,2)"}"#)
    );
    assert_eq!(field.as_deref(), Some("quote \"d\""));
    assert_eq!(empty.as_deref(), Some("{}"));
}

#[test]
fn should_reject_an_element_that_is_not_a_record_of_the_type() {
    // Arrange
    let fixture = fixture("composite_array_bad_element");

    // Act
    let bad_field = fixture.error(r#"SELECT '{"(a,notint)"}'::address[]"#);
    let extra_field = fixture.error(r#"SELECT '{"(a,1,2)"}'::address[]"#);
    let unbalanced = fixture.error(r#"SELECT '{"(a,1)"'::address[]"#);

    // Assert
    assert_eq!(bad_field.sql_state(), "22P02");
    assert_eq!(extra_field.sql_state(), "22P02");
    assert_eq!(unbalanced.sql_state(), "22P02");
}

#[test]
fn should_round_trip_a_composite_array_column() {
    // Arrange
    let fixture = fixture("composite_array_column");
    fixture
        .execute(
            "INSERT INTO trips (id, stops) VALUES \
             (1, ARRAY[ROW('1 Main St', 10)::address, NULL, ROW(NULL, 30)::address]), \
             (2, '{}'), \
             (3, NULL), \
             (4, '{\"(x,4)\"}')",
        )
        .unwrap();

    // Act
    let stops = first_column_text(&fixture.rows("SELECT stops::text FROM trips ORDER BY id"));
    let zip = text(
        &fixture,
        "SELECT (stops[3]).zip::text FROM trips WHERE id = 1",
    );
    let length = text(
        &fixture,
        "SELECT array_length(stops, 1)::text FROM trips WHERE id = 1",
    );

    // Assert
    assert_eq!(
        stops,
        vec![
            Some(r#"{"(\"1 Main St\",10)",NULL,"(,30)"}"#.to_owned()),
            Some("{}".to_owned()),
            None,
            Some(r#"{"(x,4)"}"#.to_owned()),
        ]
    );
    assert_eq!(zip.as_deref(), Some("30"));
    assert_eq!(length.as_deref(), Some("3"));
}

#[test]
fn should_carry_a_composite_array_as_a_composite_attribute() {
    // Arrange
    let fixture = fixture("composite_array_attribute");
    fixture
        .execute(
            "INSERT INTO trips (id, plan) VALUES \
             (1, ROW('north', ARRAY[ROW('a', 1)::address, ROW('b c', 2)::address])::route)",
        )
        .unwrap();

    // Act
    let plan = text(&fixture, "SELECT plan::text FROM trips WHERE id = 1");
    let nested = text(
        &fixture,
        "SELECT ((plan).stops[2]).street FROM trips WHERE id = 1",
    );
    let reparsed = text(
        &fixture,
        r#"SELECT ('(north,"{""(a,1)"",""(\\""b c\\"",2)""}")'::route).stops::text"#,
    );

    // Assert
    assert_eq!(
        plan.as_deref(),
        Some(r#"(north,"{""(a,1)"",""(\\""b c\\"",2)""}")"#)
    );
    assert_eq!(nested.as_deref(), Some("b c"));
    assert_eq!(reparsed.as_deref(), Some(r#"{"(a,1)","(\"b c\",2)"}"#));
}

#[test]
fn should_compare_composite_arrays_element_by_element() {
    // Arrange
    let fixture = fixture("composite_array_compare");
    let verdict = |condition: &str| {
        text(
            &fixture,
            &format!("SELECT CASE WHEN {condition} THEN 'yes' ELSE 'no' END"),
        )
    };

    // Act
    let equal = verdict("ARRAY[ROW('a', 1)::address] = '{\"(a,1)\"}'::address[]");
    let different = verdict("ARRAY[ROW('a', 1)::address] = ARRAY[ROW('a', 2)::address]");
    let ordered = verdict("ARRAY[ROW('a', 1)::address] < ARRAY[ROW('a', 2)::address]");

    // Assert
    assert_eq!(equal.as_deref(), Some("yes"));
    assert_eq!(different.as_deref(), Some("no"));
    assert_eq!(ordered.as_deref(), Some("yes"));
}

#[test]
fn should_describe_a_composite_array_column_in_the_catalogs() {
    // Arrange
    let fixture = fixture("composite_array_catalog");

    // Act
    let column = fixture.rows(
        "SELECT data_type, udt_name FROM information_schema.columns \
         WHERE table_name = 'trips' AND column_name = 'stops'",
    );
    let element = text(
        &fixture,
        "SELECT e.typname FROM pg_type a JOIN pg_type e ON e.oid = a.typelem \
         WHERE a.typname = '_address'",
    );
    let attribute_type = text(
        &fixture,
        "SELECT t.typname FROM pg_attribute att \
         JOIN pg_class c ON c.oid = att.attrelid_oid \
         JOIN pg_type t ON t.oid = att.atttypid \
         WHERE c.relname = 'trips' AND att.attname = 'stops'",
    );

    // Assert
    assert_eq!(first_column_text(&column), vec![Some("ARRAY".to_owned())]);
    assert_eq!(
        column[0].get(1),
        Some(&cassie::types::Value::String("_address".to_owned()))
    );
    assert_eq!(element.as_deref(), Some("address"));
    assert_eq!(attribute_type.as_deref(), Some("_address"));
}

#[test]
fn should_keep_a_type_alive_while_an_array_column_uses_it() {
    // Arrange
    let fixture = sql_fixture(
        "composite_array_dependency",
        &[
            "CREATE TYPE address AS (street text, zip int)",
            "CREATE TABLE trips (id int, stops address[])",
        ],
    );

    // Act
    let blocked = fixture.error("DROP TYPE address");
    fixture.execute("DROP TABLE trips").unwrap();
    let dropped = fixture.execute("DROP TYPE address");

    // Assert
    assert_eq!(blocked.sql_state(), "2BP01");
    assert!(dropped.is_ok(), "the type drops once no column uses it");
}

#[test]
fn should_restore_composite_array_values_after_restart() {
    // Arrange
    let store = LocalStore::new("composite_array_restart");
    {
        let cassie = store.open();
        for sql in SETUP {
            run(&cassie, sql);
        }
        run(
            &cassie,
            "INSERT INTO trips VALUES (1, ARRAY[ROW('a b', 1)::address, NULL], \
             ROW('north', ARRAY[ROW('c', 2)::address])::route)",
        );
        cassie.shutdown();
    }

    // Act
    let cassie = store.open();
    let stops = only_text(&run(&cassie, "SELECT stops::text FROM trips"));
    let plan = only_text(&run(&cassie, "SELECT plan::text FROM trips"));
    let street = only_text(&run(&cassie, "SELECT (stops[1]).street FROM trips"));
    cassie.shutdown();

    // Assert
    assert_eq!(stops.as_deref(), Some(r#"{"(\"a b\",1)",NULL}"#));
    assert_eq!(plan.as_deref(), Some(r#"(north,"{""(c,2)""}")"#));
    assert_eq!(street.as_deref(), Some("a b"));
}
