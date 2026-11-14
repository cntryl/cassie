//! Field carriers of stored and constructed records: JSON documents and the
//! length rules of `char(n)` / `varchar(n)` fields.

use crate::support_composite_types::only_text;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn documents(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE TYPE doc AS (j jsonb, n int)",
            "CREATE TABLE docs (id int primary key, v doc)",
        ],
    );
    fixture
        .execute("INSERT INTO docs VALUES (1, ROW('{\"a\":1}'::jsonb, 1))")
        .unwrap();
    fixture
}

fn bounded(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE c3 AS (c char(3))",
            "CREATE TYPE v3 AS (c varchar(3))",
            "CREATE TABLE bounded (id int primary key, fixed c3, variable v3)",
        ],
    )
}

#[test]
fn should_keep_a_jsonb_field_of_a_stored_record_as_a_document() {
    // Arrange
    let fixture = documents("composite_jsonb_field_kind");

    // Act
    let kind = only_text(&fixture.rows("SELECT jsonb_typeof((v).j) FROM docs"));

    // Assert
    assert_eq!(kind.as_deref(), Some("object"));
}

#[test]
fn should_extract_a_key_from_a_stored_jsonb_field() {
    // Arrange
    let fixture = documents("composite_jsonb_field_extract");

    // Act
    let extracted = only_text(&fixture.rows("SELECT (v).j ->> 'a' FROM docs"));

    // Assert
    assert_eq!(extracted.as_deref(), Some("1"));
}

#[test]
fn should_match_a_stored_jsonb_field_with_containment() {
    // Arrange
    let fixture = documents("composite_jsonb_field_containment");

    // Act
    let rows = fixture.rows("SELECT id FROM docs WHERE (v).j @> '{\"a\":1}'");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_equate_a_stored_record_with_its_jsonb_constructor() {
    // Arrange
    let fixture = documents("composite_jsonb_field_equality");

    // Act
    let rows = fixture.rows("SELECT v = ROW('{\"a\":1}'::jsonb, 1)::doc FROM docs");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true)]]);
}

#[test]
fn should_render_a_stored_jsonb_field_as_jsonb_text() {
    // Arrange
    let fixture = documents("composite_jsonb_field_text");

    // Act
    let rendered = only_text(&fixture.rows("SELECT v::text FROM docs"));

    // Assert
    assert_eq!(rendered.as_deref(), Some("(\"{\"\"a\"\": 1}\",1)"));
}

#[test]
fn should_extract_a_key_from_a_jsonb_field_of_an_unstored_record() {
    // Arrange
    let fixture = documents("composite_jsonb_field_unstored");

    // Act
    let extracted = only_text(&fixture.rows("SELECT (ROW('{\"a\":1}'::jsonb, 1)::doc).j ->> 'a'"));

    // Assert
    assert_eq!(extracted.as_deref(), Some("1"));
}

#[test]
fn should_reject_an_overlong_char_field_on_insert_with_sqlstate_22001() {
    // Arrange
    let fixture = bounded("composite_char_field_overflow");

    // Act
    let error = fixture.error("INSERT INTO bounded (id, fixed) VALUES (1, ROW('abcd'))");

    // Assert
    assert_eq!(error.sql_state(), "22001");
}

#[test]
fn should_reject_an_overlong_varchar_field_on_insert_with_sqlstate_22001() {
    // Arrange
    let fixture = bounded("composite_varchar_field_overflow");

    // Act
    let error = fixture.error("INSERT INTO bounded (id, variable) VALUES (1, ROW('abcd'))");

    // Assert
    assert_eq!(error.sql_state(), "22001");
}

#[test]
fn should_reject_an_overlong_char_field_in_record_text_with_sqlstate_22001() {
    // Arrange
    let fixture = bounded("composite_char_field_text_overflow");

    // Act
    let error = fixture.error("SELECT '(abcd)'::c3");

    // Assert
    assert_eq!(error.sql_state(), "22001");
}

#[test]
fn should_discard_excess_blanks_of_a_varchar_field_on_insert() {
    // Arrange
    let fixture = bounded("composite_varchar_field_blanks");
    fixture
        .execute("INSERT INTO bounded (id, variable) VALUES (1, ROW('ab   '))")
        .unwrap();

    // Act
    let stored = only_text(&fixture.rows("SELECT (variable).c || '|' FROM bounded"));

    // Assert
    assert_eq!(stored.as_deref(), Some("ab |"));
}

#[test]
fn should_truncate_an_overlong_char_field_in_an_explicit_row_cast() {
    // Arrange
    let fixture = bounded("composite_char_field_explicit_cast");

    // Act
    let truncated = only_text(&fixture.rows("SELECT (ROW('abcd')::c3)::text"));

    // Assert
    assert_eq!(truncated.as_deref(), Some("(abc)"));
}
