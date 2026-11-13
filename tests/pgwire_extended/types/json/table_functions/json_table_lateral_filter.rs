//! Outer-only predicates reach a lateral generated source inside derived tables (#1248).

use super::json_records_support::{assert_failure, assert_lateral_rows, assert_rows};

const DERIVED: &str = "SELECT id FROM (SELECT t.id, e.value FROM \
    (VALUES (1, '[1]'::jsonb), (2, '3'::jsonb)) AS t(id, d), \
    LATERAL jsonb_array_elements(t.d) AS e WHERE t.id = 1) AS s";

#[test]
fn should_skip_failing_lateral_rows_given_an_outer_only_filter_in_a_derived_table() {
    // Arrange
    // Act
    // Assert
    assert_rows(DERIVED, &[&[Some("1")]]);
}

#[test]
fn should_skip_failing_lateral_rows_given_an_outer_only_filter_in_a_set_operation_branch() {
    // Arrange
    let sql = "SELECT 0 UNION ALL SELECT t.id FROM \
        (VALUES (1, '[1]'::jsonb), (2, '3'::jsonb)) AS t(id, d), \
        LATERAL jsonb_array_elements(t.d) AS e WHERE t.id = 1";
    // Act
    // Assert
    assert_rows(sql, &[&[Some("0")], &[Some("1")]]);
}

#[test]
fn should_still_raise_given_the_failing_row_is_selected_in_a_derived_table() {
    // Arrange
    let sql = DERIVED.replace("t.id = 1", "t.id = 2");
    // Act
    // Assert
    assert_failure(&sql, "22023", "cannot extract elements from a scalar");
}

const INNER: &str = "lf_inner AS a, LATERAL jsonb_array_elements(a.d) AS e";

#[test]
fn should_keep_outer_correlated_conjuncts_given_a_lateral_derived_table() {
    // Arrange
    let sql = format!(
        "SELECT t.id, s.v FROM lf_outer AS t, LATERAL \
         (SELECT e.value::text::int AS v FROM {INNER} WHERE a.id = t.id) AS s ORDER BY 1"
    );
    // Act
    // Assert
    assert_lateral_rows(&sql, &[&[Some("1"), Some("10")], &[Some("2"), Some("20")]]);
}

#[test]
fn should_resolve_outer_columns_given_a_set_operation_branch_in_a_lateral_derived_table() {
    // Arrange
    let sql = format!(
        "SELECT t.id, s.v FROM lf_outer AS t, LATERAL \
         (SELECT 0 AS v WHERE false \
          UNION ALL SELECT e.value::text::int FROM {INNER} WHERE a.id = t.id) AS s \
         ORDER BY 1"
    );
    // Act
    // Assert
    assert_lateral_rows(&sql, &[&[Some("1"), Some("10")], &[Some("2"), Some("20")]]);
}

#[test]
fn should_resolve_outer_columns_given_both_set_operation_branches_in_a_lateral_derived_table() {
    // Arrange
    let sql = format!(
        "SELECT t.id, s.v FROM lf_outer AS t, LATERAL \
         (SELECT e.value::text::int AS v FROM {INNER} WHERE a.id = t.id \
          UNION ALL SELECT e.value::text::int FROM {INNER} WHERE a.id = t.id) AS s \
         ORDER BY 1"
    );
    // Act
    // Assert
    assert_lateral_rows(
        &sql,
        &[
            &[Some("1"), Some("10")],
            &[Some("1"), Some("10")],
            &[Some("2"), Some("20")],
            &[Some("2"), Some("20")],
        ],
    );
}

#[test]
fn should_keep_outer_correlated_conjuncts_given_a_correlated_scalar_subquery() {
    // Arrange
    let sql = format!(
        "SELECT t.id, (SELECT max(e.value::text::int) FROM {INNER} WHERE a.id = t.id) \
         FROM lf_outer AS t ORDER BY 1"
    );
    // Act
    // Assert
    assert_lateral_rows(&sql, &[&[Some("1"), Some("10")], &[Some("2"), Some("20")]]);
}
