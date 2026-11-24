//! SQL record ordering compares array fields structurally and preserves integer keys.

use crate::fixture::{person, started, SOCIAL_SETUP};
use cassie::types::Value;

#[test]
fn should_order_reference_keys_by_exact_numeric_value() {
    // Arrange
    let fixture = started("graph-reference-numeric-order", &SOCIAL_SETUP);
    fixture.run("INSERT INTO people VALUES(10,'ten'),(9007199254740992,'large')");
    let small = person(2);
    let ten = person(10);
    let large = person(9_007_199_254_740_992);
    let larger = person(9_007_199_254_740_993);

    // Act
    let ordered = fixture.rows(&format!("SELECT {small} < {ten}, {large} < {larger}"));
    let rows = fixture.rows(
        "SELECT id FROM (SELECT id,cassie_graph.vertex_ref('social','person',\
        ARRAY[cassie_graph.key_part(id)]) AS ref FROM people WHERE id IN (2,10)) AS t ORDER BY ref",
    );

    // Assert
    assert_eq!(ordered, vec![vec![Value::Bool(true), Value::Bool(true)]]);
    assert_eq!(rows, vec![vec![Value::Int64(2)], vec![Value::Int64(10)]]);
}

#[test]
fn should_compare_array_fields_of_ordinary_composites_using_sql_array_order() {
    // Arrange
    let fixture = started(
        "graph-reference-generic-record-array",
        &["CREATE TYPE array_record AS (items INT[])"],
    );

    // Act
    let rows = fixture.rows("SELECT \
        ROW(ARRAY[2])::array_record < ROW(ARRAY[10])::array_record, \
        ROW(ARRAY[1,NULL])::array_record > ROW(ARRAY[1,2])::array_record, \
        ROW('[0:1]={1,2}'::int[])::array_record < ROW('[1:2]={1,2}'::int[])::array_record, \
        ROW('[1:1][9:10]={{1,2}}'::int[])::array_record < ROW('[1:2][0:0]={{1},{2}}'::int[])::array_record, \
        ROW(ARRAY[1,2])::array_record < ROW(ARRAY[ARRAY[1,2]])::array_record, \
        ROW('[9:9][0:0][0:3]={{{1,2,3,4}}}'::int[])::array_record \
          < ROW('[0:0][0:1][0:1]={{{1,2},{3,4}}}'::int[])::array_record");

    // Assert
    assert_eq!(rows, vec![vec![Value::Bool(true); 6]]);
}

#[test]
fn should_preserve_record_array_key_consistency_for_nondefault_bounds() {
    // Arrange
    let fixture = started(
        "graph-reference-array-key-equality",
        &[
            "CREATE TYPE array_record AS (items INT[])",
            "CREATE TABLE records(r array_record)",
            "INSERT INTO records VALUES(ROW('[0:1]={1,2}'::int[])::array_record),\
         (ROW('[0:1]={1,2}'::int[])::array_record),(ROW('[1:2]={1,2}'::int[])::array_record)",
        ],
    );

    // Act
    let equal = fixture
        .rows("SELECT count(*) FROM records WHERE r=ROW('[0:1]={1,2}'::int[])::array_record");
    let distinct = fixture.rows("SELECT count(DISTINCT r) FROM records");
    let groups = fixture.rows("SELECT count(*) FROM (SELECT r FROM records GROUP BY r) AS grouped");

    // Assert
    assert_eq!(equal, vec![vec![Value::Int64(2)]]);
    assert_eq!(distinct, vec![vec![Value::Int64(2)]]);
    assert_eq!(groups, vec![vec![Value::Int64(2)]]);
}
