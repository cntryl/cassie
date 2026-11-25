//! Renaming a domain rewrites the cast targets of dependent views, and only
//! those, whatever literal forms precede them.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

fn rows_after_rename(label: &str, view: &str) -> Vec<Vec<Value>> {
    let fixture = sql_fixture(
        label,
        &[
            "CREATE DOMAIN dom AS INT",
            "CREATE TABLE dt (id INT)",
            "INSERT INTO dt VALUES (1)",
            view,
        ],
    );
    fixture.execute("ALTER DOMAIN dom RENAME TO dom2").unwrap();
    fixture.rows("SELECT * FROM dv")
}

#[test]
fn should_rename_a_cast_after_an_escape_string_with_an_escaped_quote() {
    // Arrange
    let view = "CREATE VIEW dv AS SELECT E'\\'' AS q, '::dom' AS s, 5::dom AS x FROM dt";

    // Act
    let rows = rows_after_rename("domain_rename_escape_string", view);

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("'".into()),
            Value::String("::dom".into()),
            Value::Int64(5)
        ]]
    );
}

#[test]
fn should_keep_a_dollar_quoted_cast_text_while_renaming_a_domain() {
    // Arrange
    let view =
        "CREATE VIEW dv AS SELECT $$ a::dom $$ AS d, $t$ x::dom $t$ AS e, 5::dom AS x FROM dt";

    // Act
    let rows = rows_after_rename("domain_rename_dollar_quote", view);

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String(" a::dom ".into()),
            Value::String(" x::dom ".into()),
            Value::Int64(5)
        ]]
    );
}

#[test]
fn should_rename_a_cast_function_target_after_an_escape_string() {
    // Arrange
    let view = "CREATE VIEW dv AS SELECT E'\\'' AS q, CAST(5 AS dom) AS x FROM dt";

    // Act
    let rows = rows_after_rename("domain_rename_cast_function", view);

    // Assert
    assert_eq!(rows, vec![vec![Value::String("'".into()), Value::Int64(5)]]);
}

#[test]
fn should_rename_a_schema_qualified_cast_target_after_an_escape_string() {
    // Arrange
    let view = "CREATE VIEW dv AS SELECT E'\\'' AS q, 5::public.dom AS x FROM dt";

    // Act
    let rows = rows_after_rename("domain_rename_qualified_cast", view);

    // Assert
    assert_eq!(rows, vec![vec![Value::String("'".into()), Value::Int64(5)]]);
}
