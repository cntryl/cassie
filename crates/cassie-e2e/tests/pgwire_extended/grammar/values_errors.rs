//! PostgreSQL VALUES source arity, aliases and type-resolution errors.

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
            .expect("expected PostgreSQL ErrorResponse")
            .1,
    );
    assert!(
        fields
            .iter()
            .any(|(kind, value)| *kind == 'M' && value == expected_message),
        "{context} ErrorResponse fields: {fields:?}"
    );
}

#[test]
fn should_return_width_errors_given_a_valid_values_control_when_values_rows_have_different_arity() {
    // Arrange
    let batches = run_script(
        "values-row-arity-errors",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame("VALUES (1, 2), (3, 4)")]),
            Step::ThroughReady(vec![wire::simple_query_frame("VALUES (1, 2), (3)")]),
        ],
    );

    // Act
    let control = &batches[0];
    let mismatched_rows = &batches[1];

    // Assert
    assert_eq!(wire::error_code(control), None);
    assert_eq!(
        wire::data_rows(control),
        vec![
            vec![Some("1".to_owned()), Some("2".to_owned())],
            vec![Some("3".to_owned()), Some("4".to_owned())],
        ]
    );
    assert_error(
        mismatched_rows,
        "42601",
        "VALUES lists must all be the same length",
        "unequal VALUES row width",
    );
}

#[test]
fn should_return_alias_count_errors_given_a_valid_values_control_when_too_many_aliases_are_declared(
) {
    // Arrange
    let batches = run_script(
        "values-alias-count-errors",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT * FROM (VALUES (1, 2)) AS source(a, b)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT * FROM (VALUES (1, 2)) AS source(a, b, c)",
            )]),
        ],
    );

    // Act
    let control = &batches[0];
    let excess_aliases = &batches[1];

    // Assert
    assert_eq!(wire::error_code(control), None);
    assert_eq!(
        wire::data_rows(control),
        vec![vec![Some("1".to_owned()), Some("2".to_owned())]]
    );
    assert_error(
        excess_aliases,
        "42P10",
        "table \"source\" has 2 columns available but 3 columns specified",
        "too many derived VALUES aliases",
    );
}

#[test]
fn should_return_type_errors_given_a_valid_values_control_when_column_types_are_incompatible() {
    // Arrange
    let batches = run_script(
        "values-column-type-errors",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "VALUES (1::INT4), (2::INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "VALUES (1::INT4), ('x'::TEXT)",
            )]),
        ],
    );

    // Act
    let control = &batches[0];
    let incompatible_types = &batches[1];

    // Assert
    assert_eq!(wire::error_code(control), None);
    assert_eq!(
        wire::data_rows(control),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
    assert_error(
        incompatible_types,
        "42804",
        "VALUES types integer and text cannot be matched",
        "incompatible VALUES column types",
    );
}

#[test]
fn should_reject_values_column_type_conflicts_given_unknown_parameters_when_parse_flush_runs() {
    // Arrange
    let cases = [
        (
            "VALUES ($1), (1::INT4), ('x'::TEXT)",
            "42804",
            "VALUES types integer and text cannot be matched",
        ),
        (
            "VALUES ($1,$1),(1::BIGINT,'x'::TEXT)",
            "42P08",
            "inconsistent types deduced for parameter $1",
        ),
    ];
    // Act
    let outcomes = cases.map(|(sql, state, message)| {
        (
            run_script(
                "values-parse-conflict",
                vec![Step::ParseFlush(wire::parse_frame_with_types(
                    "conflict",
                    sql,
                    &[0],
                ))],
            ),
            state,
            message,
        )
    });
    // Assert
    for (batches, state, message) in outcomes {
        assert_error(&batches[0], state, message, "VALUES Parse conflict");
    }
}
