//! PostgreSQL math overload resolution and domain errors over pgwire.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

struct OverloadCase {
    control_sql: &'static str,
    control_types: &'static [i32],
    control_values: &'static [&'static str],
    control_result: &'static str,
    rejected_sql: &'static str,
    rejected_types: &'static [i32],
    rejected_signature: &'static str,
}

fn error_response_fields(frames: &[(u8, Vec<u8>)]) -> Vec<(char, String)> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .unwrap_or_default()
}

fn assert_overload_case(batches: &[Frames], offset: usize, case: &OverloadCase) {
    assert_eq!(batches[offset + 3].first().map(|(tag, _)| *tag), Some(b'1'));
    let rejected = &batches[offset + 4];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("42883"));
    assert!(error_response_fields(rejected)
        .iter()
        .any(|(kind, message)| *kind == 'M' && message.starts_with(case.rejected_signature)));
    assert!(!rejected
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));

    assert_eq!(batches[offset].first().map(|(tag, _)| *tag), Some(b'1'));
    let control_description = &batches[offset + 1];
    assert_eq!(
        wire::error_code(control_description),
        None,
        "control Describe: {:?}",
        error_response_fields(control_description)
    );
    let control_row = control_description
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("valid sibling-overload RowDescription");
    assert_eq!(wire::parse_row_description(&control_row.1)[0].type_oid, 701);
    let control_result = &batches[offset + 2];
    assert_eq!(
        wire::error_code(control_result),
        None,
        "control Execute: {:?}",
        error_response_fields(control_result)
    );
    assert_eq!(
        wire::data_rows(control_result),
        vec![vec![Some(case.control_result.to_owned())]]
    );

    assert_eq!(wire::error_code(&batches[offset + 5]), None);
    assert_eq!(
        wire::data_rows(&batches[offset + 5]),
        vec![vec![Some("45".to_owned())]]
    );
}

#[test]
fn should_return_overload_errors_given_supported_math_controls_when_undefined_signatures_are_prepared(
) {
    // Arrange
    let cases = [
        OverloadCase {
            control_sql: "SELECT ROUND($1)",
            control_types: &[701],
            control_values: &["1.5"],
            control_result: "2",
            rejected_sql: "SELECT ROUND($1, $2)",
            rejected_types: &[701, 23],
            rejected_signature: "function round(double precision, integer) does not exist",
        },
        OverloadCase {
            control_sql: "SELECT TRUNC($1)",
            control_types: &[701],
            control_values: &["1.5"],
            control_result: "1",
            rejected_sql: "SELECT TRUNC($1, $2)",
            rejected_types: &[701, 23],
            rejected_signature: "function trunc(double precision, integer) does not exist",
        },
        OverloadCase {
            control_sql: "SELECT LOG($1)",
            control_types: &[701],
            control_values: &["10"],
            control_result: "1",
            rejected_sql: "SELECT LOG($1, $2)",
            rejected_types: &[701, 701],
            rejected_signature: "function log(double precision, double precision) does not exist",
        },
    ];
    let mut steps = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let control = format!("issue_813_math_control_{index}");
        let control_portal = format!("issue_813_math_control_portal_{index}");
        let rejected = format!("issue_813_math_rejected_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &control,
            case.control_sql,
            case.control_types,
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&control),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame(&control_portal, &control, case.control_values),
            wire::describe_portal_frame(&control_portal),
            wire::execute_frame(&control_portal),
            wire::sync_frame(),
        ]));
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &rejected,
            case.rejected_sql,
            case.rejected_types,
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&rejected),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 45 AS recovery",
        )]));
    }

    // Act
    let batches = run_script("issue-813-math-overload-errors", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() * 6);
    for (index, case) in cases.iter().enumerate() {
        assert_overload_case(&batches, index * 6, case);
    }
}

#[test]
fn should_return_22012_given_a_supported_div_control_when_the_divisor_is_zero() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT DIV(9, 2) AS quotient",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT DIV(9, 0) AS quotient",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
    ];

    // Act
    let batches = run_script("issue-813-div-zero-error", steps);

    // Assert
    let rejected = &batches[1];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("22012"));
    assert!(error_response_fields(rejected)
        .iter()
        .any(|(kind, message)| *kind == 'M' && message == "division by zero"));
    assert_eq!(wire::data_rows(rejected), Vec::<Vec<Option<String>>>::new());

    let control = &batches[0];
    assert_eq!(
        wire::error_code(control),
        None,
        "valid DIV control: {:?}",
        error_response_fields(control)
    );
    let description = control
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("DIV result RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1700
    );
    assert_eq!(wire::data_rows(control), vec![vec![Some("4".to_owned())]]);

    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("45".to_owned())]]
    );
}
