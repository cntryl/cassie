//! Attribute names and the definitions that may not reference a record field.

use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TYPE kk AS (k text, v int)",
            "CREATE TABLE kt (id int primary key, n kk)",
        ],
    )
}

#[test]
fn should_keep_a_quoted_attribute_name_distinct_from_its_folded_form() {
    // Arrange
    let fixture = fixture("composite_attribute_case");
    fixture
        .execute("CREATE TYPE cased AS (a int, \"A\" int)")
        .unwrap();

    // Act
    let rows = fixture.rows("SELECT (ROW(1, 2)::cased).\"A\"");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_add_an_attribute_differing_only_in_quoted_case() {
    // Arrange
    let fixture = fixture("composite_attribute_case_add");

    // Act
    let added = fixture.execute("ALTER TYPE kk ADD ATTRIBUTE \"K\" int");

    // Assert
    assert!(added.is_ok());
}

#[test]
fn should_reject_a_check_on_a_record_field_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("composite_field_check");

    // Act
    let error = fixture.error("ALTER TABLE kt ADD CONSTRAINT positive CHECK ((n).v > 0)");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}

#[test]
fn should_reject_an_index_on_a_record_field_with_sqlstate_0a000() {
    // Arrange
    let fixture = fixture("composite_field_index");

    // Act
    let error = fixture.error("CREATE INDEX kt_k ON kt (((n).k))");

    // Assert
    assert_eq!(error.sql_state(), "0A000");
}
