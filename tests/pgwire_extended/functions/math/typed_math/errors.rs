//! PostgreSQL 18.6 residual scalar-function identity and rounding contracts.

use super::*;

fn error_response_fields(frames: &[(u8, Vec<u8>)]) -> Vec<(char, String)> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .unwrap_or_default()
}

#[test]
fn should_reject_two_argument_float8_log_with_postgresql_sqlstate() {
    // Arrange
    let unary_statement = "float8_log_one_argument";
    // Act
    let baseline = run_script(
        "postgresql18-log-float8-unary-baseline",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                unary_statement,
                "SELECT LOG($1) AS logarithm",
                &[701],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(unary_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("float8_log_unary_portal", unary_statement, &["10"]),
                wire::describe_portal_frame("float8_log_unary_portal"),
                wire::execute_frame("float8_log_unary_portal"),
                wire::sync_frame(),
            ]),
        ],
    );
    let two_argument_statement = "float8_log_two_arguments";
    let rejected = run_script(
        "postgresql18-log-float8-two-argument-overload",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                two_argument_statement,
                "SELECT LOG($1, $2)",
                &[701, 701],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(two_argument_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
        ],
    );

    // Assert
    let unary_description = &baseline[1];
    let unary_result = &baseline[2];
    // PostgreSQL 18.6 resolves the undefined overload during Parse analysis,
    // so the ErrorResponse answers Parse itself (oracle-corrections.md).
    let describe = &rejected[0];
    assert_eq!(rejected[0].first().map(|(tag, _)| *tag), Some(b'E'));
    assert_eq!(
        [
            wire::error_code(unary_description),
            wire::error_code(unary_result),
            wire::error_code(describe),
        ],
        [None, None, Some("42883".to_owned())],
        "unary LOG baseline and two-argument LOG ErrorResponse fields: {:?} / {:?} / {:?}",
        error_response_fields(unary_description),
        error_response_fields(unary_result),
        error_response_fields(describe)
    );
    let unary_column = &wire::parse_row_description(
        &unary_description
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("unary LOG statement RowDescription")
            .1,
    )[0];
    assert_eq!((unary_column.type_oid, unary_column.type_size), (701, 8));
    assert_eq!(
        wire::data_rows(unary_result),
        vec![vec![Some("1".to_owned())]]
    );
    let error = describe
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("undefined two-argument float8 LOG ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(kind, message)| {
            *kind == 'M'
                && message
                    .starts_with("function log(double precision, double precision) does not exist")
        }));
    assert!(!describe
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&rejected[2]), None);
    assert_eq!(
        wire::data_rows(&rejected[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

include!("metadata.rs");

include!("semantics.rs");
