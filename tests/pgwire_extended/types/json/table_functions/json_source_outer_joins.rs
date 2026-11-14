//! RIGHT and FULL joins preserve the unmatched rows of a set-returning FROM source (#829).
//! Expected rows come from PostgreSQL 18.6.

use super::json_records_support::assert_rows;

const LEFT: &str = "(VALUES (1), (2)) AS a(x)";

#[test]
fn should_keep_unmatched_recordset_rows_given_a_full_join() {
    // Arrange
    let sql = format!(
        "SELECT a.x, b.j FROM {LEFT} FULL JOIN \
         jsonb_to_recordset('[{{\"j\":2}},{{\"j\":3}}]') AS b(j int) ON a.x = b.j ORDER BY 1, 2"
    );
    // Act
    // Assert
    assert_rows(
        &sql,
        &[
            &[Some("1"), None],
            &[Some("2"), Some("2")],
            &[None, Some("3")],
        ],
    );
}

#[test]
fn should_keep_unmatched_recordset_rows_given_a_right_join() {
    // Arrange
    let sql = format!(
        "SELECT a.x, b.j FROM {LEFT} RIGHT JOIN \
         jsonb_to_recordset('[{{\"j\":2}},{{\"j\":3}}]') AS b(j int) ON a.x = b.j ORDER BY 1, 2"
    );
    // Act
    // Assert
    assert_rows(&sql, &[&[Some("2"), Some("2")], &[None, Some("3")]]);
}

#[test]
fn should_keep_unmatched_array_elements_given_a_right_join() {
    // Arrange
    let sql = format!(
        "SELECT a.x, e.value FROM {LEFT} RIGHT JOIN jsonb_array_elements('[2,3]') AS e \
         ON a.x = e.value::text::int ORDER BY 1, 2"
    );
    // Act
    // Assert
    assert_rows(&sql, &[&[Some("2"), Some("2")], &[None, Some("3")]]);
}

#[test]
fn should_keep_unmatched_object_members_given_a_full_join() {
    // Arrange
    let sql = "SELECT a.x, k.key FROM (VALUES ('a'), ('b')) AS a(x) FULL JOIN \
               jsonb_each('{\"b\":1,\"c\":2}') AS k ON a.x = k.key ORDER BY 1, 2";
    // Act
    // Assert
    assert_rows(
        sql,
        &[
            &[Some("a"), None],
            &[Some("b"), Some("b")],
            &[None, Some("c")],
        ],
    );
}

#[test]
fn should_keep_unmatched_json_table_rows_given_a_full_join() {
    // Arrange
    let sql = format!(
        "SELECT a.x, t.j FROM {LEFT} FULL JOIN \
         JSON_TABLE('[{{\"j\":2}},{{\"j\":3}}]'::jsonb, '$[*]' COLUMNS (j int)) AS t \
         ON a.x = t.j ORDER BY 1, 2"
    );
    // Act
    // Assert
    assert_rows(
        &sql,
        &[
            &[Some("1"), None],
            &[Some("2"), Some("2")],
            &[None, Some("3")],
        ],
    );
}

#[test]
fn should_keep_unmatched_populated_records_given_a_right_join() {
    // Arrange
    let sql = format!(
        "SELECT a.x, r.a FROM {LEFT} RIGHT JOIN \
         jsonb_populate_recordset(NULL::pair, '[{{\"a\":2}},{{\"a\":3}}]') AS r \
         ON a.x = r.a ORDER BY 1, 2"
    );
    // Act
    // Assert
    assert_rows(&sql, &[&[Some("2"), Some("2")], &[None, Some("3")]]);
}

#[test]
fn should_keep_unmatched_series_values_given_a_full_join() {
    // Arrange
    let sql = format!(
        "SELECT a.x, g FROM {LEFT} FULL JOIN generate_series(2, 3) AS g ON a.x = g ORDER BY 1, 2"
    );
    // Act
    // Assert
    assert_rows(
        &sql,
        &[
            &[Some("1"), None],
            &[Some("2"), Some("2")],
            &[None, Some("3")],
        ],
    );
}

#[test]
fn should_keep_unmatched_unnest_values_given_a_right_join() {
    // Arrange
    let sql = format!(
        "SELECT a.x, u FROM {LEFT} RIGHT JOIN unnest(ARRAY[2, 3]) AS u ON a.x = u ORDER BY 1, 2"
    );
    // Act
    // Assert
    assert_rows(&sql, &[&[Some("2"), Some("2")], &[None, Some("3")]]);
}
