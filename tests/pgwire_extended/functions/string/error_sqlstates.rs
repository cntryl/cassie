//! PostgreSQL string-function error SQLSTATEs for #803.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_22023_given_a_zero_field_position_when_split_part_is_called() {
    // Arrange
    let batches = run_script(
        "split-part-zero-position-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b', ',', 1) AS field",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT split_part('a,b', ',', 0) AS field",
            )]),
        ],
    );

    // Act
    let valid_control = &batches[0];
    let zero_position = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid_control), None);
    assert_eq!(
        wire::data_rows(valid_control),
        vec![vec![Some("a".to_owned())]]
    );
    assert_eq!(
        wire::error_code(zero_position).as_deref(),
        Some("22023"),
        "split_part zero-position ErrorResponse: {:?}",
        zero_position
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let fields = wire::parse_error_fields(
        &zero_position
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("split_part zero-position ErrorResponse")
            .1,
    );
    assert!(fields
        .iter()
        .any(|(kind, value)| { *kind == 'M' && value == "field position must not be zero" }));
}

#[test]
fn should_return_22004_given_a_null_identifier_when_identifier_formatting_is_requested() {
    // Arrange
    let batches = run_script(
        "format-null-identifier-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', 'field') AS identifier",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT format('%I', NULL::TEXT) AS identifier",
            )]),
        ],
    );

    // Act
    let valid_control = &batches[0];
    let null_identifier = &batches[1];

    // Assert
    assert_eq!(wire::error_code(valid_control), None);
    assert_eq!(
        wire::data_rows(valid_control),
        vec![vec![Some("\"field\"".to_owned())]]
    );
    assert_eq!(
        wire::error_code(null_identifier).as_deref(),
        Some("22004"),
        "format NULL-identifier ErrorResponse: {:?}",
        null_identifier
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let fields = wire::parse_error_fields(
        &null_identifier
            .iter()
            .find(|frame| frame.0 == b'E')
            .expect("format NULL-identifier ErrorResponse")
            .1,
    );
    assert!(fields.iter().any(|(kind, value)| {
        *kind == 'M' && value == "null values cannot be formatted as an SQL identifier"
    }));
}
