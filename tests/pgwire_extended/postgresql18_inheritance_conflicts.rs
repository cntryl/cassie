//! PostgreSQL inheritance merge conflicts and SQLSTATEs tracked by #909.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_report_42804_for_a_multi_parent_column_type_conflict() {
    // Arrange
    let batches = run_script(
        "issue-909-inherited-column-type-conflict",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_type_parent_a (shared INTEGER)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_type_parent_b (shared TEXT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_type_child () \
                 INHERITS (issue_909_type_parent_a, issue_909_type_parent_b)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45")]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let conflict = &batches[2];
    let recovery = &batches[3];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none())
    );
    assert_eq!(wire::error_code(conflict).as_deref(), Some("42804"));
    let error = conflict
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("inherited column type-conflict ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|field| field.0 == 'M')
            .map(|(_, message)| message.as_str()),
        Some("inherited column \"shared\" has a type conflict")
    );
    assert_eq!(
        fields
            .iter()
            .find(|field| field.0 == 'D')
            .map(|(_, detail)| detail.as_str()),
        Some("integer versus text")
    );
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_report_42710_for_same_named_inherited_checks_with_different_expressions() {
    // Arrange
    let batches = run_script(
        "issue-909-inherited-check-definition-conflict",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_check_parent_a ( \
                    value INTEGER CONSTRAINT issue_909_shared_check CHECK (value > 0) \
                 )",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_check_parent_b ( \
                    value INTEGER CONSTRAINT issue_909_shared_check CHECK (value < 0) \
                 )",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_check_child () \
                 INHERITS (issue_909_check_parent_a, issue_909_check_parent_b)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45")]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let conflict = &batches[2];
    let recovery = &batches[3];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none())
    );
    assert_eq!(wire::error_code(conflict).as_deref(), Some("42710"));
    let error = conflict
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("inherited check-definition ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|field| field.0 == 'M')
            .map(|(_, message)| message.as_str()),
        Some(
            "check constraint name \"issue_909_shared_check\" appears multiple times but with different expressions"
        )
    );
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_report_42611_for_conflicting_inherited_defaults() {
    // Arrange
    let batches = run_script(
        "issue-909-inherited-default-conflict",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_default_parent_a (value INTEGER DEFAULT 1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_default_parent_b (value INTEGER DEFAULT 2)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_909_default_child () \
                 INHERITS (issue_909_default_parent_a, issue_909_default_parent_b)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45")]),
        ],
    );

    // Act
    let setup = &batches[..2];
    let conflict = &batches[2];
    let recovery = &batches[3];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none())
    );
    assert_eq!(wire::error_code(conflict).as_deref(), Some("42611"));
    let error = conflict
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("inherited default-conflict ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);
    assert_eq!(
        fields
            .iter()
            .find(|field| field.0 == 'M')
            .map(|(_, message)| message.as_str()),
        Some("column \"value\" inherits conflicting default values")
    );
    assert_eq!(
        fields
            .iter()
            .find(|field| field.0 == 'H')
            .map(|(_, hint)| hint.as_str()),
        Some("To resolve the conflict, specify a default explicitly.")
    );
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}
