//! A keyword ends at any non-identifier character, so it may touch a delimiter as in PostgreSQL.
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE ja (id INT, a INT, b INT, name TEXT)",
            "CREATE TABLE jb (id INT, name TEXT)",
            "INSERT INTO ja VALUES (1, 1, 2, 'x'), (2, 2, 2, 'y')",
            "INSERT INTO jb VALUES (1, 'p'), (2, 'q')",
        ],
    )
}

fn ids(rows: Vec<Vec<Value>>) -> Vec<i64> {
    rows.into_iter()
        .map(|row| match row.first() {
            Some(Value::Int64(value)) => *value,
            other => panic!("expected an integer cell, got {other:?}"),
        })
        .collect()
}

#[test]
fn should_parse_inner_join_on_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-join-on");

    // Act
    let rows =
        fixture.rows("SELECT ja.id, jb.name FROM ja JOIN jb ON(ja.id = jb.id) ORDER BY ja.id");

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(1), Value::String("p".into())],
            vec![Value::Int64(2), Value::String("q".into())],
        ]
    );
}

#[test]
fn should_parse_left_join_on_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-left-join-on");

    // Act
    let rows = fixture.rows("SELECT ja.id FROM ja LEFT JOIN jb ON(ja.id = jb.id) ORDER BY ja.id");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_right_join_on_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-right-join-on");

    // Act
    let rows = fixture.rows("SELECT jb.id FROM ja RIGHT JOIN jb ON(ja.id = jb.id) ORDER BY jb.id");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_full_join_on_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-full-join-on");

    // Act
    let rows = fixture.rows("SELECT ja.id FROM ja FULL JOIN jb ON(ja.id = jb.id) ORDER BY ja.id");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_join_on_between_a_derived_alias_then_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-derived-join-on");

    // Act
    let rows = fixture
        .rows("SELECT t.id FROM (SELECT id FROM ja) t JOIN jb ON(t.id = jb.id) ORDER BY t.id");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_where_after_a_join_on_clause_that_closes_with_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-on-then-where");

    // Act
    let rows = fixture
        .rows("SELECT ja.id FROM ja JOIN jb ON(ja.id = jb.id)WHERE(ja.a = 1) ORDER BY ja.id");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_a_second_join_after_a_closing_parenthesis() {
    // Arrange
    let fixture = fixture("kw-join-after-paren");

    // Act
    let rows = fixture.rows(
        "SELECT ja.id FROM ja JOIN jb ON(ja.id = jb.id)JOIN jb AS j2 ON(j2.id = ja.id) ORDER BY ja.id",
    );

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_conjunction_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-and-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a = 1 AND(b = 2)");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_lowercase_conjunction_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-and-lower");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a=1 and(b=2)");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_or_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-or-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a = 5 OR(b = 2) ORDER BY id");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_conjunction_between_two_parenthesized_predicates() {
    // Arrange
    let fixture = fixture("kw-paren-and-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE(a = 1)AND(b = 2)");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_or_between_two_parenthesized_predicates() {
    // Arrange
    let fixture = fixture("kw-paren-or-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE(a = 1)OR(b = 1)");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_conjunction_after_a_closing_parenthesis_before_not() {
    // Arrange
    let fixture = fixture("kw-paren-and-not");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE(a = 1)AND NOT(b = 3)");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_conjunction_after_an_in_subquery_closing_parenthesis() {
    // Arrange
    let fixture = fixture("kw-in-subquery-and");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a IN (SELECT a FROM ja)AND b = 2");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_in_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-in-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a IN(1,2) ORDER BY id");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_not_in_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-not-in-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a NOT IN(1)");

    // Assert
    assert_eq!(ids(rows), vec![2]);
}

#[test]
fn should_parse_between_bounds_that_touch_their_keywords() {
    // Arrange
    let fixture = fixture("kw-between-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a BETWEEN(1) AND(2) ORDER BY id");

    // Assert
    assert_eq!(ids(rows), vec![1, 2]);
}

#[test]
fn should_parse_or_after_is_null_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-is-null-or");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a IS NULL OR(a = 1)");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_conjunction_after_is_not_null_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-is-not-null-and");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a IS NOT NULL AND(a = 1)");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_is_distinct_from_directly_before_a_parenthesis() {
    // Arrange
    let fixture = fixture("kw-distinct-from-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE name IS DISTINCT FROM('x')");

    // Assert
    assert_eq!(ids(rows), vec![2]);
}

#[test]
fn should_parse_conjunction_directly_before_a_string_literal() {
    // Arrange
    let fixture = fixture("kw-and-string");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a = 1 AND'x' = name");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_conjunction_directly_before_a_quoted_identifier() {
    // Arrange
    let fixture = fixture("kw-and-quoted");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE a = 1 AND\"b\" = 2");

    // Assert
    assert_eq!(ids(rows), vec![1]);
}

#[test]
fn should_parse_where_order_by_limit_touching_parentheses() {
    // Arrange
    let fixture = fixture("kw-clauses-paren");

    // Act
    let rows = fixture.rows("SELECT id FROM ja WHERE(a >= 1) ORDER BY(id) DESC LIMIT(1)");

    // Assert
    assert_eq!(ids(rows), vec![2]);
}

#[test]
fn should_parse_case_when_then_touching_parentheses() {
    // Arrange
    let fixture = fixture("kw-case-paren");

    // Act
    let rows = fixture.rows("SELECT CASE WHEN(a = 1) THEN(10) ELSE(20) END FROM ja ORDER BY id");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]);
}

#[test]
fn should_not_split_identifiers_that_contain_a_keyword() {
    // Arrange
    let fixture = sql_fixture(
        "kw-identifiers",
        &[
            "CREATE TABLE kw (onion INT, band INT, iron INT, origin INT)",
            "INSERT INTO kw VALUES (1, 2, 3, 4)",
        ],
    );

    // Act
    let rows = fixture.rows(
        "SELECT onion, band, iron FROM kw WHERE onion = 1 AND band = 2 AND iron = 3 OR origin = 9",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::Int64(1), Value::Int64(2), Value::Int64(3)]]
    );
}

#[test]
fn should_not_split_keywords_inside_string_literals() {
    // Arrange
    let fixture = fixture("kw-string-literal");

    // Act
    let rows = fixture.rows("SELECT 'x AND(y) OR(z) ON(w)' AS text");

    // Assert
    assert_eq!(
        rows,
        vec![vec![Value::String("x AND(y) OR(z) ON(w)".into())]]
    );
}

#[test]
fn should_not_split_keywords_inside_quoted_identifiers() {
    // Arrange
    let fixture = sql_fixture(
        "kw-quoted-identifier",
        &[
            "CREATE TABLE kq (\"a AND(b)\" INT, \"on(x)\" INT)",
            "INSERT INTO kq VALUES (1, 2)",
        ],
    );

    // Act
    let rows = fixture.rows("SELECT \"a AND(b)\", \"on(x)\" FROM kq");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(2)]]);
}
