//! Composite columns, ROW constructors, casts and field access.

use crate::support_composite_types::{first_column_text, only_text};
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE address AS (street text, zip int)",
            "CREATE TYPE person AS (name text, home address)",
            "CREATE TABLE people (id int, p person, a address)",
        ],
    )
}

fn text(fixture: &SqlFixture, sql: &str) -> Option<String> {
    only_text(&fixture.rows(sql))
}

#[test]
fn should_round_trip_composite_columns_through_insert_and_select() {
    // Arrange
    let fixture = fixture("composite_round_trip");
    fixture
        .execute(
            "INSERT INTO people VALUES \
             (1, ROW('Ann', ROW('1 Main St', 12345)), ROW('x y', 1)), \
             (2, NULL, ROW(NULL, NULL)), \
             (3, ROW('Bob', NULL), NULL)",
        )
        .unwrap();

    // Act
    let people = first_column_text(&fixture.rows("SELECT p::text FROM people ORDER BY id"));
    let addresses = first_column_text(&fixture.rows("SELECT a::text FROM people ORDER BY id"));

    // Assert
    assert_eq!(
        people,
        vec![
            Some("(Ann,\"(\"\"1 Main St\"\",12345)\")".to_owned()),
            None,
            Some("(Bob,)".to_owned()),
        ]
    );
    assert_eq!(
        addresses,
        vec![Some("(\"x y\",1)".to_owned()), Some("(,)".to_owned()), None]
    );
}

#[test]
fn should_insert_text_literals_and_update_composite_columns() {
    // Arrange
    let fixture = fixture("composite_update");
    fixture
        .execute("INSERT INTO people (id, a) VALUES (1, '(old,1)')")
        .unwrap();

    // Act
    fixture
        .execute("UPDATE people SET a = ROW('new', 2) WHERE id = 1")
        .unwrap();
    let updated = text(&fixture, "SELECT a::text FROM people WHERE id = 1");

    // Assert
    assert_eq!(updated, Some("(new,2)".to_owned()));
}

#[test]
fn should_distinguish_sql_null_records_from_all_null_field_records() {
    // Arrange
    let fixture = fixture("composite_null_vs_all_null");
    fixture
        .execute("INSERT INTO people (id, a) VALUES (1, NULL), (2, ROW(NULL, NULL))")
        .unwrap();

    // Act
    let rendered = first_column_text(&fixture.rows("SELECT a::text FROM people ORDER BY id"));
    let present = fixture.rows("SELECT id FROM people WHERE a IS NOT DISTINCT FROM NULL ORDER BY id");

    // Assert
    assert_eq!(rendered, vec![None, Some("(,)".to_owned())]);
    assert_eq!(present, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_apply_postgresql_is_null_rules_to_records() {
    // Arrange
    let fixture = fixture("composite_is_null");
    fixture
        .execute(
            "INSERT INTO people (id, p) VALUES \
             (1, ROW('Ann', ROW('1 Main', 1))), (2, NULL), (3, ROW('Bob', NULL))",
        )
        .unwrap();

    // Act
    let is_null = fixture.rows("SELECT id FROM people WHERE p IS NULL ORDER BY id");
    let is_not_null = fixture.rows("SELECT id FROM people WHERE p IS NOT NULL ORDER BY id");

    // Assert
    assert_eq!(is_null, vec![vec![Value::Int64(2)]]);
    assert_eq!(
        is_not_null,
        vec![vec![Value::Int64(1)]],
        "a partly NULL record is neither NULL nor NOT NULL"
    );
}

#[test]
fn should_apply_is_null_rules_to_row_constructors() {
    // Arrange
    let fixture = sql_fixture("composite_row_is_null", &[]);

    // Act
    let rows = fixture.rows(
        "SELECT ROW(1, 'x') IS NULL, ROW(NULL, NULL) IS NULL, \
         ROW(NULL, 1) IS NULL, ROW(NULL, 1) IS NOT NULL",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false)
        ]]
    );
}

#[test]
fn should_select_fields_from_expressions_and_columns() {
    // Arrange
    let fixture = fixture("composite_field_access");
    fixture
        .execute(
            "INSERT INTO people VALUES \
             (1, ROW('Ann', ROW('1 Main St', 12345)), ROW('x y', 1)), (2, NULL, NULL)",
        )
        .unwrap();

    // Act
    let names = first_column_text(&fixture.rows("SELECT (p).name FROM people ORDER BY id"));
    let streets = first_column_text(&fixture.rows("SELECT (people.a).street FROM people ORDER BY id"));
    let nested = fixture.rows("SELECT ((p).home).zip FROM people ORDER BY id");
    let expression = text(&fixture, "SELECT (ROW('a', 1)::address).street");

    // Assert
    assert_eq!(names, vec![Some("Ann".to_owned()), None]);
    assert_eq!(streets, vec![Some("x y".to_owned()), None]);
    assert_eq!(nested, vec![vec![Value::Int64(12345)], vec![Value::Null]]);
    assert_eq!(expression, Some("a".to_owned()));
}

#[test]
fn should_reject_unknown_fields_and_non_composite_operands() {
    // Arrange
    let fixture = fixture("composite_field_errors");

    // Act
    let unknown = fixture.error("SELECT (a).missing FROM people");
    let scalar = fixture.error("SELECT (id).street FROM people");

    // Assert
    assert_eq!(unknown.sql_state(), "42703");
    assert_eq!(scalar.sql_state(), "42809");
}

#[test]
fn should_cast_row_constructors_and_text_literals_to_named_types() {
    // Arrange
    let fixture = fixture("composite_casts");

    // Act
    let from_row = text(&fixture, "SELECT ROW('a', 1)::address::text");
    let implicit = text(&fixture, "SELECT ('a', 1)::address::text");
    let from_text = text(&fixture, "SELECT '(\"a b\",2)'::address::text");
    let all_null = text(&fixture, "SELECT '(,)'::address::text");

    // Assert
    assert_eq!(from_row, Some("(a,1)".to_owned()));
    assert_eq!(implicit, Some("(a,1)".to_owned()));
    assert_eq!(from_text, Some("(\"a b\",2)".to_owned()));
    assert_eq!(all_null, Some("(,)".to_owned()));
}

#[test]
fn should_report_postgresql_errors_for_arity_mismatched_row_casts() {
    // Arrange
    let fixture = fixture("composite_cast_arity");

    // Act
    let too_few = fixture.error("SELECT ROW(1)::address");
    let too_many = fixture.error("SELECT ROW(1, 2, 3)::address");

    // Assert
    assert_eq!(too_few.sql_state(), "42846");
    assert_eq!(too_many.sql_state(), "42846");
}

#[test]
fn should_report_malformed_record_literals_with_sqlstate_22p02() {
    // Arrange
    let fixture = fixture("composite_malformed_literals");

    // Act
    let errors = [
        fixture.error("SELECT '(a)'::address"),
        fixture.error("SELECT '(a,1,2)'::address"),
        fixture.error("SELECT 'a,1'::address"),
        fixture.error("SELECT '(a,1)x'::address"),
    ];

    // Assert
    for error in errors {
        assert_eq!(error.sql_state(), "22P02");
    }
}

#[test]
fn should_reject_anonymous_records_as_stored_columns() {
    // Arrange
    let fixture = sql_fixture("composite_anonymous_column", &[]);

    // Act
    let error = fixture.error("CREATE TABLE bad (r record)");

    // Assert
    assert!(
        matches!(error.sql_state(), "42704" | "42P16" | "0A000"),
        "RECORD must not be a column type"
    );
}

#[test]
fn should_reject_indexes_and_unique_constraints_on_composite_columns() {
    // Arrange
    let fixture = fixture("composite_no_index");

    // Act
    let index = fixture.error("CREATE INDEX people_a ON people (a)");
    let unique = fixture.error("CREATE TABLE u (a address UNIQUE)");

    // Assert
    assert_eq!(index.sql_state(), "0A000");
    assert_eq!(unique.sql_state(), "0A000");
}
