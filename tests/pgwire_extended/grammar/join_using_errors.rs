//! PostgreSQL JOIN USING resolution and duplicate-column errors.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn assert_error(
    frames: &[(u8, Vec<u8>)],
    expected_state: &str,
    expected_message: &str,
    context: &str,
) {
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some(expected_state),
        "{context} ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let fields = wire::parse_error_fields(
        &frames
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("expected JOIN USING ErrorResponse")
            .1,
    );
    assert!(
        fields
            .iter()
            .any(|(kind, value)| *kind == 'M' && value == expected_message),
        "{context} ErrorResponse fields: {fields:?}"
    );
}

fn setup_steps() -> Vec<Step> {
    vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_800_using_left (id INT4, left_value TEXT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_800_using_right (id INT4, right_value TEXT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_800_using_left VALUES (1, 'left')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_800_using_right VALUES (1, 'right')",
        )]),
    ]
}

#[test]
fn should_return_missing_column_errors_given_valid_join_sources_when_using_names_an_absent_column()
{
    // Arrange
    let mut steps = setup_steps();
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT id FROM issue_800_using_left JOIN issue_800_using_right USING (id)",
    )]));
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT id FROM issue_800_using_left JOIN issue_800_using_right USING (missing)",
    )]));
    let batches = run_script("join-using-missing-column", steps);

    // Act
    let setup = &batches[..4];
    let control = &batches[4];
    let missing_column = &batches[5];

    // Assert
    for (index, frames) in setup.iter().enumerate() {
        assert_eq!(
            wire::error_code(frames),
            None,
            "JOIN USING setup step {index} ErrorResponse: {:?}",
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(wire::error_code(control), None);
    assert_eq!(wire::data_rows(control), vec![vec![Some("1".to_owned())]]);
    assert_error(
        missing_column,
        "42703",
        "column \"missing\" specified in USING clause does not exist in left table",
        "JOIN USING missing column",
    );
}

#[test]
fn should_return_duplicate_column_errors_given_valid_join_sources_when_using_repeats_a_column() {
    // Arrange
    let mut steps = setup_steps();
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT id FROM issue_800_using_left JOIN issue_800_using_right USING (id)",
    )]));
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT id FROM issue_800_using_left JOIN issue_800_using_right USING (id, id)",
    )]));
    let batches = run_script("join-using-duplicate-column", steps);

    // Act
    let setup = &batches[..4];
    let control = &batches[4];
    let duplicate_column = &batches[5];

    // Assert
    for (index, frames) in setup.iter().enumerate() {
        assert_eq!(
            wire::error_code(frames),
            None,
            "JOIN USING setup step {index} ErrorResponse: {:?}",
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(wire::error_code(control), None);
    assert_eq!(wire::data_rows(control), vec![vec![Some("1".to_owned())]]);
    assert_error(
        duplicate_column,
        "42701",
        "column name \"id\" appears more than once in USING clause",
        "JOIN USING duplicate column",
    );
}
