//! `INSERT ... ON CONFLICT DO UPDATE` over composite columns.

use crate::support_composite_types::only_text;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE TYPE pair AS (k text, v int)",
            "CREATE TABLE kv (id int primary key, c pair, n int)",
        ],
    );
    fixture
        .execute("INSERT INTO kv VALUES (1, ROW('a', 1), 10)")
        .unwrap();
    fixture
}

#[test]
fn should_assign_the_excluded_record_of_a_conflicting_insert() {
    // Arrange
    let fixture = fixture("composite_upsert_excluded_record");

    // Act
    fixture
        .execute(
            "INSERT INTO kv VALUES (1, ROW('z', 26), 0) \
             ON CONFLICT (id) DO UPDATE SET c = EXCLUDED.c",
        )
        .unwrap();
    let stored = only_text(&fixture.rows("SELECT c::text FROM kv WHERE id = 1"));

    // Assert
    assert_eq!(stored.as_deref(), Some("(z,26)"));
}

#[test]
fn should_select_a_field_of_the_excluded_record() {
    // Arrange
    let fixture = fixture("composite_upsert_excluded_field");

    // Act
    fixture
        .execute(
            "INSERT INTO kv VALUES (1, ROW('z', 26), 0) \
             ON CONFLICT (id) DO UPDATE SET n = (EXCLUDED.c).v",
        )
        .unwrap();
    let rows = fixture.rows("SELECT n FROM kv WHERE id = 1");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(26)]]);
}
