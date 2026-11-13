//! PostgreSQL 18.6 `NESTED PATH` semantics of `JSON_TABLE`: sibling union and outer joins.

use super::json_records_support::assert_rows;

#[test]
fn should_union_sibling_nested_paths_given_two_nested_clauses() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a\":[1,2]},{\"a\":[3]}]'::jsonb, '$[*]' COLUMNS (\
               i FOR ORDINALITY, \
               NESTED PATH '$.a[*]' COLUMNS (j FOR ORDINALITY, x int PATH '$'), \
               NESTED PATH '$.a[0]' COLUMNS (y int PATH '$')))";
    // Act
    // Assert
    assert_rows(
        sql,
        &[
            &[Some("1"), Some("1"), Some("1"), None],
            &[Some("1"), Some("2"), Some("2"), None],
            &[Some("1"), None, None, Some("1")],
            &[Some("2"), Some("1"), Some("3"), None],
            &[Some("2"), None, None, Some("3")],
        ],
    );
}

#[test]
fn should_keep_the_parent_row_given_an_empty_nested_path() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a\":1},{\"a\":2,\"b\":[]}]'::jsonb, '$[*]' COLUMNS (\
               a int PATH '$.a', NESTED PATH '$.b[*]' COLUMNS (y int PATH '$')))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), None], &[Some("2"), None]]);
}

#[test]
fn should_expand_each_nested_item_given_a_populated_nested_path() {
    // Arrange
    let sql = "SELECT * FROM JSON_TABLE('[{\"a\":1,\"b\":[7,8]}]'::jsonb, '$[*]' COLUMNS (\
               a int PATH '$.a', NESTED PATH '$.b[*]' COLUMNS (y int PATH '$')))";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("7")], &[Some("1"), Some("8")]]);
}
