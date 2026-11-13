//! Outer-only predicates reach a lateral generated source inside derived tables (#1248).

use super::json_records_support::{assert_failure, assert_rows};

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
