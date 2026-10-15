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
fn should_report_postgresql_math_result_oids_across_function_overloads() {
    // Arrange
    let statement = "math_numeric_float8_overloads";
    let portal = "math_numeric_float8_overloads_portal";
    let sql = "SELECT ROUND($1, $3) AS numeric_round, ROUND($2) AS float8_round, \
        TRUNC($1, $3) AS numeric_trunc, FLOOR($1) AS numeric_floor, \
        FLOOR($2) AS float8_floor, CEILING($1) AS numeric_ceiling, \
        CEILING($2) AS float8_ceiling";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[1700, 701, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["1.25", "2.5", "1"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-math-result-identities", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(
        wire::error_code(described),
        None,
        "math overload Describe ErrorResponse fields: {:?}",
        described
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let parameter_types = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("math overload ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![1700, 701, 23]
    );
    let description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("math overload statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 701, 1700, 1700, 701, 1700, 701]
    );
    let result = &batches[2];
    assert_eq!(
        wire::error_code(result),
        None,
        "math overload Execute ErrorResponse fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("1.3".to_owned()),
            Some("2".to_owned()),
            Some("1.2".to_owned()),
            Some("1".to_owned()),
            Some("2".to_owned()),
            Some("2".to_owned()),
            Some("3".to_owned()),
        ]]
    );
}

#[test]
fn should_round_float8_midpoints_to_even() {
    // Arrange
    let statement = "float8_rounding_midpoints";
    let portal = "float8_rounding_midpoints_portal";
    let sql = "SELECT ROUND($1), ROUND($2), ROUND($3), ROUND($4)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[701, 701, 701, 701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["2.5", "3.5", "-2.5", "-3.5"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-rounding-tie-distinction", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("rounding tie RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701, 701, 701]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("2".to_owned()),
            Some("4".to_owned()),
            Some("-2".to_owned()),
            Some("-4".to_owned()),
        ]]
    );
}

#[test]
fn should_distinguish_float8_round_scale_from_numeric_round() {
    // Arrange
    let statement = "float8_round_scale";
    // Act
    let baseline = run_script(
        "postgresql18-round-numeric-overload-baseline",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROUND(1.25, 1) AS rounded_value",
        )])],
    );
    let rejected = run_script(
        "postgresql18-round-float8-scale-overload",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                statement,
                "SELECT ROUND($1, $2)",
                &[701, 23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
        ],
    );

    // Assert
    let baseline_result = &baseline[0];
    let describe = &rejected[1];
    assert_eq!(rejected[0].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(
        [
            wire::error_code(baseline_result),
            wire::error_code(describe),
        ],
        [None, Some("42883".to_owned())],
        "numeric ROUND baseline and float8 ROUND overload ErrorResponse fields: {:?} / {:?}",
        error_response_fields(baseline_result),
        error_response_fields(describe)
    );
    let baseline_description = baseline_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric ROUND RowDescription");
    assert_eq!(
        wire::parse_row_description(&baseline_description.1)[0].type_oid,
        1700
    );
    assert_eq!(
        wire::data_rows(baseline_result),
        vec![vec![Some("1.3".to_owned())]]
    );
    let error = describe
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("undefined float8 ROUND overload ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(kind, message)| {
            *kind == 'M'
                && message.starts_with("function round(double precision, integer) does not exist")
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
fn should_distinguish_float8_trunc_scale_from_numeric_trunc() {
    // Arrange
    let statement = "float8_trunc_scale";
    // Act
    let baseline = run_script(
        "postgresql18-trunc-numeric-overload-baseline",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TRUNC(1.29, 1) AS truncated_value",
        )])],
    );
    let rejected = run_script(
        "postgresql18-trunc-float8-scale-overload",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                statement,
                "SELECT TRUNC($1, $2)",
                &[701, 23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
        ],
    );

    // Assert
    let baseline_result = &baseline[0];
    let describe = &rejected[1];
    assert_eq!(rejected[0].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(
        [
            wire::error_code(baseline_result),
            wire::error_code(describe),
        ],
        [None, Some("42883".to_owned())],
        "numeric TRUNC baseline and float8 TRUNC overload ErrorResponse fields: {:?} / {:?}",
        error_response_fields(baseline_result),
        error_response_fields(describe)
    );
    let baseline_description = baseline_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric TRUNC RowDescription");
    assert_eq!(
        wire::parse_row_description(&baseline_description.1)[0].type_oid,
        1700
    );
    assert_eq!(
        wire::data_rows(baseline_result),
        vec![vec![Some("1.2".to_owned())]]
    );
    let error = describe
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("undefined float8 TRUNC overload ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(kind, message)| {
            *kind == 'M'
                && message.starts_with("function trunc(double precision, integer) does not exist")
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
    let describe = &rejected[1];
    assert_eq!(rejected[0].first().map(|(tag, _)| *tag), Some(b'1'));
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
fn should_describe_untyped_ceiling_literal_as_numeric() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT CEILING(42.8) AS rounded_value",
    )])];

    // Act
    let batches = run_script("postgresql18-ceiling-untyped-numeric-descriptor", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("CEILING literal RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (1700, -1, -1)
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("43".to_owned())]]);
}

#[test]
fn should_report_division_by_zero_from_div() {
    // Arrange
    // Act
    let baseline = run_script(
        "postgresql18-div-nonzero-baseline",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT DIV(9, 2) AS integer_quotient",
        )])],
    );
    let rejected = run_script(
        "postgresql18-div-zero-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT DIV(9, 0)")]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 43 AS recovery")]),
        ],
    );

    // Assert
    let baseline_result = &baseline[0];
    let zero_result = &rejected[0];
    assert_eq!(
        [
            wire::error_code(baseline_result),
            wire::error_code(zero_result),
        ],
        [None, Some("22012".to_owned())],
        "DIV nonzero baseline and zero-divisor ErrorResponse fields: {:?} / {:?}",
        error_response_fields(baseline_result),
        error_response_fields(zero_result)
    );
    let description = baseline_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("nonzero DIV RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1700
    );
    assert_eq!(
        wire::data_rows(baseline_result),
        vec![vec![Some("4".to_owned())]]
    );
    assert!(!zero_result
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&rejected[1]), None);
    assert_eq!(
        wire::data_rows(&rejected[1]),
        vec![vec![Some("43".to_owned())]]
    );
}

#[test]
fn should_preserve_exact_numeric_power_result_over_pgwire() {
    // Arrange
    let statement = "exact_numeric_power";
    let portal = "exact_numeric_power_portal";
    let sql = "SELECT POWER($1, $2) AS squared";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[1700, 1700])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["9007199254740993", "2"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-exact-numeric-power-result", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(
        wire::error_code(described),
        None,
        "numeric POWER Describe error fields: {:?}",
        described
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("numeric POWER ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![1700, 1700]
    );
    let description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric POWER statement RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (1700, -1, -1)
    );
    let result = &batches[2];
    assert_eq!(
        wire::error_code(result),
        None,
        "numeric POWER execution error fields: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("81129638414606699710187514626049".to_owned())]]
    );
}
