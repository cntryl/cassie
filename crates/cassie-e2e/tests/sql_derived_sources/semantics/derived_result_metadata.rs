use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

const TEXT_OID: i64 = 25;
const INT4_OID: i64 = 23;
const FLOAT8_OID: i64 = 701;
const BOOL_OID: i64 = 16;

fn typed_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE t (name TEXT, n INT, f FLOAT, flag BOOLEAN)",
            "CREATE TABLE other (a INT, c TEXT)",
            "INSERT INTO t (name, n, f, flag) VALUES ('x', 7, 1.5, true)",
            "INSERT INTO other (a, c) VALUES (9, 'other-nine')",
        ],
    )
}

fn column_oids(fixture: &SqlFixture, sql: &str) -> Vec<(String, i64)> {
    let Ok(result) = fixture.execute(sql) else {
        panic!("query failed: {sql}");
    };
    result
        .columns()
        .iter()
        .map(|column| (column.name.clone(), column.type_oid))
        .collect()
}

#[test]
fn should_describe_derived_table_columns_with_their_real_types() {
    // Arrange
    let fixture = typed_fixture("derived_types");
    let expected = vec![
        ("n".to_string(), INT4_OID),
        ("f".to_string(), FLOAT8_OID),
        ("flag".to_string(), BOOL_OID),
    ];

    // Act
    let derived = column_oids(&fixture, "SELECT n, f, flag FROM (SELECT * FROM t) d");
    let described = fixture
        .cassie
        .describe_sql("SELECT n, f, flag FROM (SELECT * FROM t) d");

    // Assert
    assert_eq!(derived, expected);
    let Ok(described) = described else {
        panic!("describe failed");
    };
    let described: Vec<(String, i64)> = described
        .into_iter()
        .map(|column| (column.name, column.type_oid))
        .collect();
    assert_eq!(described, expected);
}

#[test]
fn should_not_adopt_a_same_named_table_schema_for_an_alias() {
    // Arrange
    let fixture = typed_fixture("derived_alias_collision");
    let expected = vec![("a".to_string(), INT4_OID), ("c".to_string(), TEXT_OID)];

    // Act
    let derived = column_oids(&fixture, "SELECT * FROM (SELECT a, c FROM other) t");
    let cte = column_oids(
        &fixture,
        "WITH t AS (SELECT a, c FROM other) SELECT * FROM t",
    );
    let rows = fixture.rows("SELECT * FROM (SELECT a, c FROM other) t");

    // Assert
    assert_eq!(derived, expected);
    assert_eq!(cte, expected);
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(9),
            Value::String("other-nine".to_string())
        ]]
    );
}
