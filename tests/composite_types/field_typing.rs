//! Typing of record fields outside a plain column read: declared parameters,
//! untyped literals and the session styles of a record text cast.

use crate::support_composite_types::only_text;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE TYPE tagged AS (u uuid, b bytea, t timestamptz)",
            "CREATE TABLE tags (id int primary key, v tagged)",
        ],
    );
    fixture
        .execute(
            "INSERT INTO tags VALUES (1, ROW(\
             'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '\\xdead', '2026-07-01 12:00:00+00'))",
        )
        .unwrap();
    fixture
}

#[test]
fn should_select_a_field_of_a_declared_composite_parameter() {
    // Arrange
    let fixture = sql_fixture(
        "composite_prepare_field_access",
        &["CREATE TYPE pair AS (k text, v int)"],
    );
    fixture
        .execute("PREPARE pick(pair) AS SELECT ($1).k")
        .unwrap();

    // Act
    let selected = only_text(&fixture.rows("EXECUTE pick('(a,1)')"));

    // Assert
    assert_eq!(selected.as_deref(), Some("a"));
}

#[test]
fn should_compare_a_uuid_field_with_an_untyped_literal() {
    // Arrange
    let fixture = fixture("composite_uuid_field_literal");

    // Act
    let rows =
        fixture.rows("SELECT id FROM tags WHERE (v).u = 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_compare_a_bytea_field_with_an_untyped_literal() {
    // Arrange
    let fixture = fixture("composite_bytea_field_literal");

    // Act
    let rows = fixture.rows("SELECT id FROM tags WHERE (v).b = '\\xdead'");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_render_a_record_text_cast_in_the_session_time_zone() {
    // Arrange
    let fixture = sql_fixture(
        "composite_text_cast_time_zone",
        &[
            "CREATE TYPE stamped AS (t timestamptz)",
            "CREATE TABLE stamps (id int primary key, v stamped)",
            "INSERT INTO stamps VALUES (1, ROW('2026-07-01 12:00:00+00'))",
        ],
    );
    fixture
        .execute("SET timezone = 'America/New_York'")
        .unwrap();

    // Act
    let rendered = only_text(&fixture.rows("SELECT v::text FROM stamps"));

    // Assert
    assert_eq!(rendered.as_deref(), Some("(\"2026-07-01 08:00:00-04\")"));
}
