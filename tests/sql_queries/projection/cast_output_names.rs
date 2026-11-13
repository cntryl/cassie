//! An unaliased cast of a column is named after the column (PostgreSQL 18.6),
//! which also decides what `ORDER BY <name>` sorts.
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE cast_names (e INT, f TEXT)",
            "INSERT INTO cast_names VALUES (2, 'b'), (10, 'a')",
        ],
    )
}

fn column_names(result: &cassie::executor::QueryResult) -> Vec<String> {
    result
        .columns()
        .iter()
        .map(|column| column.name.clone())
        .collect()
}

#[test]
fn should_name_an_unaliased_column_cast_after_the_column() {
    // Arrange
    let fixture = fixture("cast-output-name");

    // Act
    let result = fixture.execute("SELECT e::text, CAST(f AS varchar), cast_names.e::bigint FROM cast_names");

    // Assert
    assert!(result.is_ok_and(|result| column_names(&result) == ["e", "f", "e"]));
}

#[test]
fn should_order_by_the_cast_output_when_its_name_is_used() {
    // Arrange
    let fixture = fixture("cast-output-order");

    // Act
    let by_text = fixture.rows("SELECT e::text FROM cast_names ORDER BY e");
    let by_number = fixture.rows("SELECT e::text AS shown FROM cast_names ORDER BY e");

    // Assert
    assert_eq!(
        by_text,
        vec![
            vec![Value::String("10".into())],
            vec![Value::String("2".into())]
        ]
    );
    assert_eq!(
        by_number,
        vec![
            vec![Value::String("2".into())],
            vec![Value::String("10".into())]
        ]
    );
}

#[test]
fn should_keep_an_explicit_alias_on_a_column_cast() {
    // Arrange
    let fixture = fixture("cast-output-explicit-alias");

    // Act
    let result = fixture.execute("SELECT e::text AS shown FROM cast_names");

    // Assert
    assert!(result.is_ok_and(|result| column_names(&result) == ["shown"]));
}
