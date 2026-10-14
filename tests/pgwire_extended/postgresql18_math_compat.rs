//! PostgreSQL 18 math-function descriptors and parameter binding tracked by #813.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_math_function_results_with_the_postgresql_types() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT CEILING(42.8), PI(), WIDTH_BUCKET(5.35, 0.024, 10.06, 5)",
    )])];

    // Act
    let batches = run_script("postgresql18-math-result-descriptors", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "wire error fields: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("math-function RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 701, 23]
    );
}

#[test]
fn should_bind_float8_rounding_parameters_after_type_inference() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "round_float8_parameter",
            "SELECT ROUND(CAST($1 AS FLOAT))",
            &[701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("round_float8_parameter"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("round_float8_portal", "round_float8_parameter", &["42.4"]),
            wire::describe_portal_frame("round_float8_portal"),
            wire::execute_frame("round_float8_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-round-float8-parameter", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let parameter_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![701]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_report_postgresql_math_error_sqlstates() {
    // Arrange
    let cases = [
        ("SELECT POWER(-1.0, 0.5)", "2201F"),
        (
            "SELECT POWER(CAST(1e308 AS FLOAT), CAST(2.0 AS FLOAT))",
            "22003",
        ),
        ("SELECT LN(0.0)", "2201E"),
        ("SELECT SETSEED(2.0)", "22023"),
    ];
    let steps = cases
        .iter()
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("postgresql18-math-domain-errors", steps);

    // Assert
    assert_eq!(batches.len(), cases.len());
    let mut mismatches = Vec::new();
    for (batch, (sql, expected_code)) in batches.iter().zip(cases) {
        let actual_code = wire::error_code(batch);
        if actual_code.as_deref() != Some(expected_code) {
            mismatches.push((
                sql,
                expected_code,
                actual_code,
                batch
                    .iter()
                    .filter(|(tag, _)| *tag == b'E')
                    .map(|(_, payload)| wire::parse_error_fields(payload))
                    .collect::<Vec<_>>(),
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "math error mismatches: {mismatches:?}"
    );
}

#[test]
fn should_reject_two_float8_argument_log_with_undefined_function_sqlstate() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT LOG(CAST(2 AS FLOAT), CAST(64 AS FLOAT))",
    )])];

    // Act
    let batches = run_script("postgresql18-log-float8-undefined", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("42883"),
        "wire error fields: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert!(!batches[0]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
}
