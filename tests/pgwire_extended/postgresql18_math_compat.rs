//! PostgreSQL 18 math-function descriptors and parameter binding tracked by #813.

use crate::support_parameter_occurrence_wire::{Step, run_script};
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
fn should_bind_numeric_round_with_postgresql_parameter_contracts() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let statement = "round_numeric_parameters";
    let sql = "SELECT ROUND($1, $2) AS rounded";
    let numeric_1245 = [
        0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x09, 0x92,
    ];
    let scale_two = 2_i32.to_be_bytes();
    let rounded_125_binary = [
        0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0x09, 0xc4,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "round_numeric_text_portal",
                statement,
                &[0],
                &[Some(&b"-1.245"[..]), Some(&b"2"[..])],
                &[0],
            ),
            wire::describe_portal_frame("round_numeric_text_portal"),
            wire::execute_frame("round_numeric_text_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "round_numeric_binary_portal",
                statement,
                &[1],
                &[Some(&numeric_1245[..]), Some(&scale_two[..])],
                &[1],
            ),
            wire::describe_portal_frame("round_numeric_binary_portal"),
            wire::execute_frame("round_numeric_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-round-numeric-parameter-formats", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameters = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("inferred numeric and integer ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![1700, 23]
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric round RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(1700, -1)]
    );

    assert_eq!(wire::error_code(&batches[2]), None);
    let text_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("text numeric round portal RowDescription");
    assert_eq!(
        wire::parse_row_description(&text_description.1)[0].format_code,
        0
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("-1.25".to_owned())]]
    );

    assert_eq!(wire::error_code(&batches[3]), None);
    let binary_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary numeric round portal RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!((binary_column.type_oid, binary_column.type_mod), (1700, -1));
    assert_eq!(binary_column.format_code, 1);
    assert_eq!(
        fixture::data_row_payloads(&batches[3]),
        vec![fixture::one_field_row(&rounded_125_binary)]
    );
}

#[test]
fn should_return_positive_zero_for_negative_float8_sign_in_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let statement = "sign_float8_parameter";
    let portal = "sign_negative_zero_portal";
    let negative_zero = [0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT SIGN($1::FLOAT8) AS sign_value",
            &[701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[1], &[Some(&negative_zero)], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-sign-negative-zero-wire", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let parameters = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("float8 ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![701]);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("float8 sign RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    fixture::assert_single_column_descriptors(&batches[2], (701, 8, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[2]),
        vec![fixture::one_field_row(&[0; 8])]
    );
}

#[test]
fn should_report_postgresql_math_error_sqlstates() {
    // Arrange
    let cases = [
        ("SELECT POWER(-1.0, 0.5)", "2201F"),
        ("SELECT WIDTH_BUCKET(1, 1, 2, 0)", "2201G"),
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
    assert!(
        !batches[0]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
    );
}
