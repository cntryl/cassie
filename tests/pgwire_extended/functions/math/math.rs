//! PostgreSQL 18 math-function descriptors and parameter binding tracked by #813.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "mod_zero_division.rs"]
mod mod_zero_division;
#[path = "numeric_log_binary.rs"]
mod numeric_log_binary;
#[path = "power_range_messages.rs"]
mod power_range_messages;
#[path = "typed_math/errors.rs"]
mod residual_typed_math_contract;

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
fn should_describe_log_overloads_with_postgresql_result_oids() {
    // Arrange
    let sql = "SELECT LOG($1) AS float8_log, LOG($2) AS numeric_log, \
              LOG($3, $4) AS numeric_base_log";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "log_overloads",
            sql,
            &[701, 1700, 1700, 1700],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("log_overloads"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(
                "log_overloads_portal",
                "log_overloads",
                &["100", "100", "2", "64"],
            ),
            wire::describe_portal_frame("log_overloads_portal"),
            wire::execute_frame("log_overloads_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-log-overload-signatures", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let statement = &batches[1];
    assert_eq!(
        wire::error_code(statement),
        None,
        "LOG Describe SQLSTATE; frames: {statement:?}"
    );
    let parameter_types = statement
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("LOG ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![701, 1700, 1700, 1700]
    );
    let statement_description = statement
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("LOG statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 1700, 1700]
    );
    let frames = &batches[2];
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("math overload RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 1700, 1700]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("2".to_owned()),
            Some("2.0000000000000000".to_owned()),
            Some("6.0000000000000000".to_owned()),
        ]]
    );
}

#[test]
fn should_describe_float8_ceiling_with_float8_result_oid() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "ceiling_float8",
            "SELECT CEILING($1)",
            &[701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("ceiling_float8"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("ceiling_float8_portal", "ceiling_float8", &["42.8"]),
            wire::describe_portal_frame("ceiling_float8_portal"),
            wire::execute_frame("ceiling_float8_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-ceiling-float8-result", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let statement = &batches[1];
    assert_eq!(
        wire::error_code(statement),
        None,
        "float8 CEILING Describe SQLSTATE; frames: {statement:?}"
    );
    let statement_description = statement
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("float8 CEILING statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701]
    );
    let frames = &batches[2];
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("float8 CEILING RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("43".to_owned())]]);
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
    // The portal batch carries only its own Describe RowDescription.
    fixture::assert_single_column_descriptors(&batches[2], (701, 8, -1), 1, false);
    assert_eq!(
        fixture::data_row_payloads(&batches[2]),
        vec![fixture::one_field_row(&[0; 8])]
    );
}

#[test]
fn should_describe_random_overloads_with_postgresql_result_oids() {
    // Arrange
    let sql = "SELECT RANDOM() AS float8_value, RANDOM(1, 10) AS int4_value, RANDOM(1::BIGINT, 10::BIGINT) AS int8_value, RANDOM(1::NUMERIC, 2::NUMERIC) AS numeric_value, RANDOM_NORMAL(4.5, 0.0) AS normal_value, RANDOM_NORMAL(4.5, NULL::FLOAT8) AS null_normal_value";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-random-result-oids", steps);

    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire error fields: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("random overload RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(701, 8), (23, 4), (20, 8), (1700, -1), (701, 8), (701, 8)]
    );
    let rows = wire::data_rows(frames);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.len(), 6);
    let random_float8 = row[0]
        .as_deref()
        .expect("random float8 value")
        .parse::<f64>()
        .expect("random float8 parses");
    assert!((0.0..1.0).contains(&random_float8));
    let random_int4 = row[1]
        .as_deref()
        .expect("random int4 value")
        .parse::<i32>()
        .expect("random int4 parses");
    assert!((1..=10).contains(&random_int4));
    let random_int8 = row[2]
        .as_deref()
        .expect("random int8 value")
        .parse::<i64>()
        .expect("random int8 parses");
    assert!((1..=10).contains(&random_int8));
    let random_numeric = row[3]
        .as_deref()
        .expect("random numeric value")
        .parse::<f64>()
        .expect("random numeric parses");
    assert!((1.0..=2.0).contains(&random_numeric));
    assert_eq!(row[4].as_deref(), Some("4.5"));
    assert_eq!(row[5], None);
}

#[test]
fn should_infer_float8_random_normal_parameters_with_zero_deviation() {
    // Arrange
    let statement = "random_normal_parameters";
    let portal = "random_normal_zero_deviation";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            statement,
            "SELECT RANDOM_NORMAL($1, $2) AS normal_value",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["4.5", "0.0"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-random-normal-parameter-contract", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let parameter_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("random_normal ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![701, 701]
    );
    let statement_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("random_normal statement RowDescription");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("4.5".to_owned())]]
    );
}

#[test]
fn should_preserve_float8_zero_sign_for_round_trunc_ceil_abs() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let statement = "float8_signed_zero_math";
    let portal = "float8_signed_zero_math_binary";
    let sql = "SELECT ROUND(CAST(-0.1 AS FLOAT8)), TRUNC(CAST(-0.1 AS FLOAT8)), CEIL(CAST(-0.1 AS FLOAT8)), ABS(CAST(-0.0 AS FLOAT8))";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-float8-signed-zero-math", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("signed-zero math RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(701, 8, 1); 4]
    );
    let mut expected = 4_i16.to_be_bytes().to_vec();
    for bits in [
        [0x80, 0, 0, 0, 0, 0, 0, 0],
        [0x80, 0, 0, 0, 0, 0, 0, 0],
        [0x80, 0, 0, 0, 0, 0, 0, 0],
        [0; 8],
    ] {
        expected.extend_from_slice(&8_i32.to_be_bytes());
        expected.extend_from_slice(&bits);
    }
    assert_eq!(fixture::data_row_payloads(&batches[1]), vec![expected]);
}

#[test]
fn should_report_postgresql_math_error_sqlstates() {
    // Arrange
    let cases = [
        ("SELECT POWER(-1.0, 0.5)", "2201F"),
        ("SELECT WIDTH_BUCKET(1, 1, 2, 0)", "2201G"),
        ("SELECT SQRT(CAST(-1 AS FLOAT8))", "2201F"),
        (
            "SELECT POWER(CAST(1e308 AS FLOAT), CAST(2.0 AS FLOAT))",
            "22003",
        ),
        ("SELECT EXP(CAST(1000 AS FLOAT8))", "22003"),
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

#[test]
fn should_report_sqrt_domain_sqlstate_for_a_bound_parameter() {
    // Arrange
    let statement = "sqrt_negative_parameter";
    let portal = "sqrt_negative_parameter_portal";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT SQRT($1::FLOAT8)",
            &[701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["-1"]),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-bound-sqrt-domain-error", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameter_types = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("float8 ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![701]
    );
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("2201F"));
    assert!(!batches[2]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[3]), None);
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_report_exp_overflow_sqlstate_for_a_bound_parameter() {
    // Arrange
    let statement = "exp_overflow_parameter";
    let portal = "exp_overflow_parameter_portal";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT EXP($1::FLOAT8)",
            &[701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["1000"]),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-bound-exp-overflow", steps);

    // Assert
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(wire::error_code(&batches[1]), None);
    let parameter_types = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("float8 ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![701]
    );
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22003"));
    assert!(!batches[2]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[3]), None);
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_infer_int4_width_bucket_count_for_typed_operands() {
    // Arrange
    let queries = [
        "SELECT WIDTH_BUCKET(0.5::float8, 0::float8, 1::float8, $1)",
        "SELECT WIDTH_BUCKET(0.5::numeric, 0::numeric, 1::numeric, $1)",
        "SELECT pg_catalog.width_bucket(0.5::float8, 0::float8, 1::float8, $1)",
    ];
    let mut steps = Vec::new();
    for (index, sql) in queries.iter().enumerate() {
        let statement = format!("issue_813_bucket_parameter_{index}");
        let text_portal = format!("issue_813_bucket_text_{index}");
        let binary_portal = format!("issue_813_bucket_binary_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame(&statement, sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&statement),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame(&text_portal, &statement, &["2"]),
            wire::execute_frame(&text_portal),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                &binary_portal,
                &statement,
                &[1],
                &[Some(&2_i32.to_be_bytes())],
                &[],
            ),
            wire::execute_frame(&binary_portal),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", &statement, &[0], &[None], &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame("", &statement, &["2147483648"]),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 45",
        )]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-count-inference", steps);

    // Assert
    for index in 0..queries.len() {
        let offset = index * 7;
        assert_eq!(wire::error_code(&batches[offset]), None);
        let described = &batches[offset + 1];
        let parameters = described
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), [23]);
        let row = described
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("row description");
        assert_eq!(wire::parse_row_description(&row.1)[0].type_oid, 23);
        for result in &batches[offset + 2..offset + 4] {
            assert_eq!(wire::error_code(result), None);
            assert_eq!(wire::data_rows(result), vec![vec![Some("2".to_owned())]]);
        }
        assert_eq!(wire::data_rows(&batches[offset + 4]), vec![vec![None]]);
        assert_eq!(
            wire::error_code(&batches[offset + 5]).as_deref(),
            Some("22003")
        );
        assert_eq!(
            wire::data_rows(&batches[offset + 6]),
            vec![vec![Some("45".to_owned())]]
        );
    }
}

#[test]
fn should_preserve_width_bucket_parameter_types_and_reject_unsupported_count_with_distinct_arity_routine(
) {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "repeated_bucket_count",
            "SELECT WIDTH_BUCKET(0.5::float8, 0::float8, 1::float8, $1), $1",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("repeated_bucket_count"),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "small_bucket_count",
            "SELECT WIDTH_BUCKET(0.5::float8, 0::float8, 1::float8, $1)",
            &[21],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("small_bucket_count"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            r#"CREATE FUNCTION width_bucket(x INT) RETURNS INT AS "x""#,
        )]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "routine_bucket_count",
            "SELECT WIDTH_BUCKET(0.5::float8, 0::float8, 1::float8, $1)",
            &[20],
        )),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];

    // Act
    let batches = run_script("issue-813-width-bucket-parameter-context", steps);

    // Assert
    for (index, expected) in [(1, 23), (3, 21)] {
        let description = batches[index]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&description.1),
            [expected]
        );
    }
    assert_eq!(wire::error_code(&batches[4]), None);
    assert_eq!(batches[5].first().map(|frame| frame.0), Some(b'E'));
    assert_eq!(wire::error_code(&batches[5]).as_deref(), Some("42883"));
}

#[test]
fn should_infer_all_unknown_width_bucket_parameters_and_repeated_operands() {
    // Arrange
    let cases: [(&str, &[i32], &[&str], &str); 2] = [
        (
            "SELECT WIDTH_BUCKET($1, $2, $3, $4)",
            &[701, 701, 701, 23],
            &["0.5", "0", "1", "2"],
            "2",
        ),
        (
            "SELECT WIDTH_BUCKET($1, $1, $2, $3)",
            &[701, 701, 23],
            &["1", "3", "2"],
            "1",
        ),
    ];
    let mut steps = Vec::new();
    for (index, (sql, _, values, _)) in cases.iter().enumerate() {
        let statement = format!("issue_813_bucket_all_unknown_{index}");
        let portal = format!("issue_813_bucket_all_unknown_portal_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame(&statement, sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(&statement),
            wire::sync_frame(),
        ]));
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame(&portal, &statement, values),
            wire::execute_frame(&portal),
            wire::sync_frame(),
        ]));
        let mut null_values: Vec<Option<&[u8]>> =
            values.iter().map(|value| Some(value.as_bytes())).collect();
        null_values[0] = None;
        steps.push(Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", &statement, &[0], &null_values, &[]),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-all-unknown", steps);

    // Assert
    for (index, (_, types, _, expected)) in cases.iter().enumerate() {
        let offset = index * 4;
        assert_eq!(batches[offset].first().map(|frame| frame.0), Some(b'1'));
        let description = batches[offset + 1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&description.1).as_slice(),
            *types
        );
        let row = batches[offset + 1]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("row description");
        assert_eq!(wire::parse_row_description(&row.1)[0].type_oid, 23);
        assert_eq!(wire::error_code(&batches[offset + 2]), None);
        assert_eq!(
            wire::data_rows(&batches[offset + 2]),
            vec![vec![Some((*expected).to_owned())]]
        );
        assert_eq!(wire::data_rows(&batches[offset + 3]), vec![vec![None]]);
    }
}

#[test]
fn should_preserve_later_casts_supplied_types_and_declared_width_bucket_routines() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "bucket_later_cast",
            "SELECT WIDTH_BUCKET($1, $2, $3, $4), $1::text",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_later_cast"),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "bucket_supplied",
            "SELECT WIDTH_BUCKET($1, $2, $3, $4)",
            &[701, 701, 701, 23],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_supplied"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            r#"CREATE FUNCTION width_bucket(a TEXT, b TEXT, c TEXT, d TEXT) RETURNS TEXT AS "d""#,
        )]),
        Step::ParseFlush(wire::parse_frame(
            "bucket_declared",
            "SELECT WIDTH_BUCKET($1, $2, $3, $4)",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_declared"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-813-width-bucket-all-unknown-context", steps);

    // Assert
    for (index, expected) in [
        (1, [701, 701, 701, 23]),
        (3, [701, 701, 701, 23]),
        (6, [25, 25, 25, 25]),
    ] {
        let parameters = batches[index]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), expected);
    }
    assert_eq!(wire::error_code(&batches[4]), None);
}
