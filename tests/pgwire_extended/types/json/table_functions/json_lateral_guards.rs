//! A `WHERE` conjunct that excludes a preserved-side row runs before the lateral
//! set-returning source expands that row, for every plan shape (#829).
//! Expected rows come from PostgreSQL 18.6; row 2 holds an object, so expanding it with
//! `jsonb_array_elements` raises `22023`.

use super::json_elements_support::{error_message, query, row, Rows};
use super::{fixture, wire};
use cassie::types::Value;

fn setup() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        (
            "CREATE TABLE lf_docs (id INT PRIMARY KEY, j JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO lf_docs VALUES (1, '[1,2]'), (2, '{\"k\":1}'), \
             (3, '[{\"a\":1},{\"b\":2}]')",
            Vec::new(),
        ),
        ("CREATE TABLE lf_sink (id INT, v JSONB)", Vec::new()),
    ]
}

fn expected(rows: &[&[Option<&str>]]) -> Rows {
    rows.iter().map(|cells| row(cells)).collect()
}

fn assert_rows(sql: &str, rows: &[&[Option<&str>]]) {
    let frames = query("json-lateral-guards", &setup(), sql, false);
    assert_eq!(error_message(&frames), None, "{sql}");
    assert_eq!(wire::data_rows(&frames), expected(rows), "{sql}");
}

/// Runs `write` and then `read` on one connection and returns the rows of `read`.
fn rows_after(write: &str, read: &str) -> Rows {
    let cycles = vec![
        fixture::execute_cycle(write, None, 0, false),
        fixture::execute_cycle(read, None, 0, false),
    ];
    let frames = fixture::run_cycles("json-lateral-guards-write", &setup(), cycles);
    assert_eq!(error_message(&frames[0]), None, "{write}");
    wire::data_rows(&frames[1])
}

const ONE: &[&[Option<&str>]] = &[&[Some("1"), Some("1")], &[Some("1"), Some("2")]];

#[test]
fn should_skip_excluded_rows_given_insert_select_over_a_lateral_source() {
    // Arrange
    let write = "INSERT INTO lf_sink SELECT d.id, e.value FROM lf_docs d, \
                 jsonb_array_elements(d.j) e WHERE d.id = 1";
    // Act
    let rows = rows_after(write, "SELECT id, v FROM lf_sink ORDER BY 2");
    // Assert
    assert_eq!(rows, expected(ONE));
}

#[test]
fn should_skip_excluded_rows_given_create_table_as_over_a_lateral_source() {
    // Arrange
    let write = "CREATE TABLE lf_copy AS SELECT d.id, e.value AS v FROM lf_docs d, \
                 jsonb_array_elements(d.j) e WHERE d.id = 1";
    // Act
    let rows = rows_after(write, "SELECT id, v FROM lf_copy ORDER BY 2");
    // Assert
    assert_eq!(rows, expected(ONE));
}

#[test]
fn should_skip_excluded_rows_given_chained_lateral_expansions() {
    // Arrange
    let sql = "SELECT d.id, f.key FROM lf_docs d, jsonb_array_elements(d.j) e, \
               jsonb_each(e.value) f WHERE d.id = 3 ORDER BY 2";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("3"), Some("a")], &[Some("3"), Some("b")]]);
}

#[test]
fn should_skip_excluded_rows_given_a_table_after_the_lateral_source() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d, jsonb_array_elements(d.j) e, lf_docs d2 \
               WHERE d.id = 1 AND d2.id = 1 ORDER BY 2";
    // Act
    // Assert
    assert_rows(sql, ONE);
}

#[test]
fn should_skip_excluded_rows_given_a_jsonb_typeof_guard() {
    // Arrange
    let sql = "SELECT d.id, k.key FROM lf_docs d, jsonb_each(d.j) k \
               WHERE jsonb_typeof(d.j) = 'object'";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("2"), Some("k")]]);
}

#[test]
fn should_skip_excluded_rows_given_a_guard_in_the_join_condition() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d JOIN LATERAL jsonb_array_elements(d.j) e \
               ON jsonb_typeof(d.j) = 'array' WHERE d.id < 3 ORDER BY 2";
    // Act
    // Assert
    assert_rows(sql, ONE);
}

#[test]
fn should_skip_excluded_rows_given_an_any_array_filter() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d, jsonb_array_elements(d.j) e \
               WHERE d.id = ANY(ARRAY[1]) ORDER BY 2";
    // Act
    // Assert
    assert_rows(sql, ONE);
}

#[test]
fn should_skip_excluded_rows_given_a_case_filter() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d, jsonb_array_elements(d.j) e \
               WHERE CASE WHEN d.id = 1 THEN true ELSE false END ORDER BY 2";
    // Act
    // Assert
    assert_rows(sql, ONE);
}

#[test]
fn should_skip_excluded_rows_given_a_coalesce_filter() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d, jsonb_array_elements(d.j) e \
               WHERE coalesce(d.id, 0) = 1 ORDER BY 2";
    // Act
    // Assert
    assert_rows(sql, ONE);
}

#[test]
fn should_skip_excluded_rows_given_a_left_join_lateral_with_an_in_filter() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d LEFT JOIN LATERAL \
               jsonb_array_elements(d.j) e ON true WHERE d.id IN (1, 3) ORDER BY 1, 2";
    // Act
    // Assert
    assert_rows(
        sql,
        &[
            &[Some("1"), Some("1")],
            &[Some("1"), Some("2")],
            &[Some("3"), Some("{\"a\": 1}")],
            &[Some("3"), Some("{\"b\": 2}")],
        ],
    );
}

#[test]
fn should_keep_unexpanded_left_rows_given_a_left_join_lateral_without_a_filter_match() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d LEFT JOIN LATERAL \
               jsonb_array_elements(d.j) e ON e.value = '2' WHERE d.id <> 2 ORDER BY 1, 2";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("2")], &[Some("3"), None]]);
}

#[test]
fn should_skip_excluded_rows_given_a_correlated_exists_over_the_outer_document() {
    // Arrange
    let sql = "SELECT d.id FROM lf_docs d WHERE d.id = 1 AND EXISTS \
               (SELECT 1 FROM jsonb_array_elements(d.j) e WHERE e.value = '2')";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1")]]);
}

#[test]
fn should_skip_excluded_rows_given_exists_before_the_plain_filter() {
    // Arrange
    let sql = "SELECT d.id FROM lf_docs d WHERE EXISTS \
               (SELECT 1 FROM jsonb_array_elements(d.j) e WHERE e.value = '2') AND d.id = 1";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1")]]);
}

#[test]
fn should_skip_inner_rows_given_an_outer_column_filter_in_a_correlated_exists() {
    // Arrange
    let sql = "SELECT d.id FROM lf_docs d WHERE d.id = 1 AND EXISTS \
               (SELECT 1 FROM lf_docs k, jsonb_array_elements(k.j) e WHERE k.id = d.id)";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1")]]);
}

#[test]
fn should_skip_inner_rows_given_an_outer_column_filter_in_a_scalar_subquery() {
    // Arrange
    let sql = "SELECT d.id, (SELECT count(*) FROM lf_docs k, jsonb_array_elements(k.j) e \
               WHERE k.id = d.id) FROM lf_docs d WHERE d.id = 1";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("1"), Some("2")]]);
}

#[test]
fn should_still_raise_given_the_failing_row_is_selected() {
    // Arrange
    let sql = "SELECT d.id, e.value FROM lf_docs d, jsonb_array_elements(d.j) e WHERE d.id = 2";
    // Act
    let frames = query("json-lateral-guards-error", &setup(), sql, false);
    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22023"));
    assert_eq!(wire::data_rows(&frames), Rows::new());
}

#[test]
fn should_type_a_table_column_named_like_a_generated_source_column_by_its_table() {
    // Arrange
    let setup = vec![
        (
            "CREATE TABLE lf_kv (id INT PRIMARY KEY, value JSONB, column1 TEXT)",
            Vec::new(),
        ),
        ("INSERT INTO lf_kv VALUES (1, '[1,2]', 'c1')", Vec::new()),
    ];
    let sql = "SELECT lf_kv.id, e.value::text FROM lf_kv, jsonb_array_elements(lf_kv.value) e \
               WHERE lf_kv.column1 = 'c1' ORDER BY 2";

    // Act
    let frames = query("json-generated-column-name", &setup, sql, false);

    // Assert
    assert_eq!(error_message(&frames), None, "{sql}");
    assert_eq!(
        wire::data_rows(&frames),
        expected(&[&[Some("1"), Some("1")], &[Some("1"), Some("2")]])
    );
}
