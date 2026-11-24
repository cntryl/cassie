//! Mixed numeric mutation operators use the observed PostgreSQL FLOAT domain.
use super::support_mixed_numeric_dml::{self as support, assert_cases, capture};

#[test]
fn should_resolve_mixed_numeric_update_filters() {
    // Arrange
    let cases = support::UPDATE;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}

#[test]
fn should_resolve_mixed_numeric_delete_filters() {
    // Arrange
    let cases = support::DELETE;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}

#[test]
fn should_resolve_mixed_numeric_insert_select_filters() {
    // Arrange
    let cases = support::INSERT_SELECT;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}

#[test]
fn should_resolve_mixed_numeric_update_assignments() {
    // Arrange
    let cases = support::UPDATE_SET;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}

#[test]
fn should_resolve_mixed_numeric_insert_select_projections() {
    // Arrange
    let cases = support::INSERT_SELECT_PROJECTION;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}

#[test]
fn should_resolve_mixed_numeric_conflict_updates() {
    // Arrange
    let cases = support::ON_CONFLICT;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}

#[test]
fn should_rollback_mixed_numeric_update_results() {
    // Arrange
    let cases = support::ROLLBACK;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}

#[test]
fn should_preserve_rows_after_a_failed_mixed_numeric_assignment() {
    // Arrange
    let cases = support::ATOMICITY;

    // Act
    let observations = capture(cases);

    // Assert
    assert_cases(cases, observations);
}
