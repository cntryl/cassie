//! PostgreSQL LIKE and SIMILAR TO invalid ESCAPE behavior.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn assert_invalid_escape(frames: &[(u8, Vec<u8>)], context: &str) {
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22025"),
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
            .expect("invalid ESCAPE ErrorResponse")
            .1,
    );
    assert!(fields
        .iter()
        .any(|(kind, value)| { *kind == 'M' && value == "invalid escape string" }));
    assert!(fields.iter().any(|(kind, value)| {
        *kind == 'H' && value == "Escape string must be empty or one character."
    }));
}

#[test]
fn should_return_22025_given_a_valid_like_control_when_escape_contains_multiple_characters() {
    // Arrange
    let batches = run_script(
        "like-multiple-character-escape-error",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 'abc' LIKE 'a%' ESCAPE '!' AS matched",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 'abc' LIKE 'a%' ESCAPE '!!' AS matched",
            )]),
        ],
    );

    // Act
    let valid_control = &batches[0];
    let invalid_escape = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid_control), None);
    assert_eq!(
        wire::data_rows(valid_control),
        vec![vec![Some("t".to_owned())]]
    );
    assert_invalid_escape(invalid_escape, "LIKE multi-character ESCAPE");
}

#[test]
fn should_return_22025_given_a_valid_similar_control_when_escape_contains_multiple_characters() {
    // Arrange
    let batches = run_script(
        "similar-multiple-character-escape-error",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 'abc' SIMILAR TO 'a%' ESCAPE '!' AS matched",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 'abc' SIMILAR TO 'a%' ESCAPE '!!' AS matched",
            )]),
        ],
    );

    // Act
    let valid_control = &batches[0];
    let invalid_escape = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid_control), None);
    assert_eq!(
        wire::data_rows(valid_control),
        vec![vec![Some("t".to_owned())]]
    );
    assert_invalid_escape(invalid_escape, "SIMILAR TO multi-character ESCAPE");
}
