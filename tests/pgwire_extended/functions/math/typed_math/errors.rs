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

#[test]
fn should_reject_float_modulo_without_a_postgresql_overload() {
    // Arrange
    let statement = "float_modulo_without_overload";
    let real = wire::parse_frame_with_types(statement, "SELECT MOD($1, $2)", &[700, 700]);
    let float8_statement = "float8_modulo_without_overload";
    let float8 = wire::parse_frame_with_types(float8_statement, "SELECT MOD($1, $2)", &[701, 701]);

    // Act
    let real_batches = run_script(
        "postgresql18-real-modulo-is-undefined",
        vec![
            Step::ParseFlush(real),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
        ],
    );
    let float8_batches = run_script(
        "postgresql18-float8-modulo-is-undefined",
        vec![
            Step::ParseFlush(float8),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(float8_statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
        ],
    );

    // Assert
    for batches in [&real_batches, &float8_batches] {
        assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'E'));
        assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
        assert!(!batches[0]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        assert_eq!(wire::error_code(&batches[2]), None);
        assert_eq!(
            wire::data_rows(&batches[2]),
            vec![vec![Some("42".to_owned())]]
        );
    }
    for (batch, expected_signature) in [
        (&real_batches[0], "function mod(real, real) does not exist"),
        (
            &float8_batches[0],
            "function mod(double precision, double precision) does not exist",
        ),
    ] {
        let error = batch
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("undefined MOD overload ErrorResponse");
        assert!(wire::parse_error_fields(&error.1)
            .iter()
            .any(|(kind, message)| *kind == 'M' && message == expected_signature));
    }
}

#[test]
fn should_reject_non_int4_types_for_numeric_rounding_scales() {
    // Arrange
    let cases = [
        ("round", 20, "numeric, bigint"),
        ("round", 1700, "numeric, numeric"),
        ("round", 701, "numeric, double precision"),
        ("trunc", 20, "numeric, bigint"),
        ("trunc", 1700, "numeric, numeric"),
        ("trunc", 701, "numeric, double precision"),
    ];

    for (index, (function, scale_oid, signature)) in cases.into_iter().enumerate() {
        let statement = format!("numeric_scale_signature_{index}");
        let sql = format!("SELECT {function}($1, $2)");
        let script = format!("postgresql18-{function}-scale-type-{index}");
        let steps = vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                &statement,
                &sql,
                &[1700, scale_oid],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(&statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
        ];

        // Act
        let batches = run_script(&script, steps);

        // Assert
        let rejected = &batches[0];
        assert_eq!(rejected.first().map(|(tag, _)| *tag), Some(b'E'));
        assert_eq!(wire::error_code(rejected).as_deref(), Some("42883"));
        let error = rejected
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("undefined numeric rounding scale ErrorResponse");
        assert!(wire::parse_error_fields(&error.1)
            .iter()
            .any(|(kind, message)| {
                *kind == 'M'
                    && message == &format!("function {function}({signature}) does not exist")
            }));
        assert!(!rejected
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
        assert_eq!(wire::error_code(&batches[2]), None);
        assert_eq!(
            wire::data_rows(&batches[2]),
            vec![vec![Some("42".to_owned())]]
        );
    }
}

#[test]
fn should_resolve_numeric_rounding_string_scales_as_int4() {
    // Arrange
    let sql = "SELECT ROUND(1.245::numeric, '2'), TRUNC(-1.249::numeric, '2')";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-numeric-rounding-string-scales", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric rounding scale RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1.25".to_owned()), Some("-1.24".to_owned())]]
    );
}

#[test]
fn should_accept_smallint_parameters_for_numeric_rounding_scales() {
    // Arrange
    let statement = "numeric_rounding_smallint_scale";
    let portal = "numeric_rounding_smallint_scale_portal";
    let sql = "SELECT ROUND($1, $2), TRUNC($1, $2)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[1700, 21])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["-1.245", "2"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-rounding-smallint-scale", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("numeric rounding ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![1700, 21]
    );
    let result_description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric rounding RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("-1.25".to_owned()), Some("-1.24".to_owned())]]
    );
}

include!("metadata.rs");

include!("semantics.rs");
