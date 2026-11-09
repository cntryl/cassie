//! PostgreSQL math overload resolution and domain errors over pgwire.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

type Frames = Vec<(u8, Vec<u8>)>;

#[test]
fn should_reject_numeric_ceiling_format_carry_without_partial_rows() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ceil((repeat('9',131072)||'.1')::numeric)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45")]),
    ];

    // Act
    let batches = run_script("numeric-ceiling-carry-overflow", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22003"));
    assert!(!batches[0]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("45".to_owned())]]
    );
}

#[test]
fn should_return_numeric_exponential_range_errors_without_publishing_prior_rows() {
    // Arrange
    let queries = [
        "SELECT exp(n) FROM (VALUES (0::numeric),(6000::numeric)) AS input(n)",
        "SELECT power(n,e) FROM (VALUES (10::numeric,2::numeric),(1e4::numeric,1000.5::numeric)) AS input(n,e)",
    ];
    let mut steps = Vec::new();
    for sql in queries {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT exp(NULL::numeric), exp(-6000::numeric) = 0, power(10::numeric,2::numeric)",
        )]));
    }

    // Act
    let batches = run_script("issue-813-numeric-exp-range", steps);

    // Assert
    for pair in batches.as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("22003"));
        assert!(!pair[0].iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
        assert_eq!(pair[0].last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(wire::error_code(&pair[1]), None);
        assert_eq!(
            wire::data_rows(&pair[1]),
            vec![vec![
                None,
                Some("t".to_owned()),
                Some("100.00000000000000".to_owned())
            ]]
        );
        let description = pair[1]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("recovery descriptors");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|field| field.type_oid)
                .collect::<Vec<_>>(),
            [1700, 16, 1700]
        );
    }
}

#[test]
fn should_reject_undefined_infinite_numeric_powers_without_partial_output() {
    // Arrange
    let queries = [
        "SELECT power(base, exponent) FROM (VALUES (1::numeric, 2::numeric), ('-Infinity'::numeric, 0.5::numeric)) AS inputs(base, exponent)",
        "SELECT power(base, exponent) FROM (VALUES (1::numeric, 2::numeric), (0::numeric, '-Infinity'::numeric)) AS inputs(base, exponent)",
        "SELECT base ^ exponent FROM (VALUES (1::numeric, 2::numeric), ('-Infinity'::numeric, 0.5::numeric)) AS inputs(base, exponent)",
        "SELECT base ^ exponent FROM (VALUES (1::numeric, 2::numeric), (0::numeric, '-Infinity'::numeric)) AS inputs(base, exponent)",
    ];
    let mut steps = Vec::new();
    for sql in queries {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT power('-Infinity'::numeric, 3::numeric), power(NULL::numeric, '-Infinity'::numeric), power('1e400'::numeric, '0.001'::numeric)",
        )]));
    }

    // Act
    let batches = run_script("issue-813-infinite-power-domains", steps);

    // Assert
    for pair in batches.as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("2201F"));
        assert!(!pair[0].iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
        assert_eq!(pair[0].last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(wire::error_code(&pair[1]), None);
        assert_eq!(
            wire::data_rows(&pair[1]),
            vec![vec![
                Some("-Infinity".to_owned()),
                None,
                Some("2.5118864315095801".to_owned())
            ]]
        );
        let description = pair[1]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("recovered numeric result descriptors");
        assert!(wire::parse_row_description(&description.1)
            .iter()
            .all(|field| field.type_oid == 1700));
    }
}

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
    // PostgreSQL 18.6 resolves the undefined overload during Parse analysis,
    // so the ErrorResponse answers Parse itself (oracle-corrections.md).
    assert_eq!(batches[offset + 3].first().map(|(tag, _)| *tag), Some(b'E'));
    let rejected = &batches[offset + 3];
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

#[test]
fn should_report_width_bucket_int4_overflow_and_keep_wire_session_usable() {
    // Arrange
    let queries = [
        "SELECT WIDTH_BUCKET(1::float8, 0::float8, 1::float8, 2147483647)",
        "SELECT WIDTH_BUCKET(0::float8, 1::float8, 0::float8, 2147483647)",
        "SELECT WIDTH_BUCKET(1::numeric, 0::numeric, 1::numeric, 2147483647)",
        "SELECT WIDTH_BUCKET(0::numeric, 1::numeric, 0::numeric, 2147483647)",
        "SELECT WIDTH_BUCKET(v, 0::float8, 1::float8, 2147483647) FROM (VALUES (0.5::float8), (1::float8)) AS inputs(v)",
    ];
    let mut steps = Vec::new();
    for query in queries {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(query)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame("SELECT WIDTH_BUCKET('0.9999999999999999'::float8, 0::float8, 1::float8, 2147483647), WIDTH_BUCKET(0.9999999999::numeric, 0::numeric, 1::numeric, 2147483647)")]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-int4-wire", steps);

    // Assert
    for pair in batches.as_chunks::<2>().0 {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("22003"));
        assert!(!pair[0].iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
        assert_eq!(pair[0].last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(wire::error_code(&pair[1]), None);
        assert_eq!(
            wire::data_rows(&pair[1]),
            vec![vec![
                Some("2147483647".to_owned()),
                Some("2147483647".to_owned())
            ]]
        );
        let description = pair[1]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("descriptors");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|field| field.type_oid)
                .collect::<Vec<_>>(),
            [23, 23]
        );
    }
}

#[test]
fn should_reject_typed_width_bucket_count_overloads_during_parse() {
    // Arrange
    let cases = [(20, "bigint"), (1700, "numeric"), (701, "double precision")];
    let mut steps = Vec::new();
    for (index, (oid, _)) in cases.iter().enumerate() {
        let name = format!("issue_813_width_bucket_bad_count_{index}");
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            &name,
            "SELECT WIDTH_BUCKET(NULL::float8, 0::float8, 1::float8, $1)",
            &[*oid],
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT WIDTH_BUCKET(0.5::float8, 0::float8, 1::float8, 1::smallint)",
        )]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-count-wire", steps);

    // Assert
    for (index, (_, kind)) in cases.iter().enumerate() {
        let error = &batches[index * 3];
        assert_eq!(error.first().map(|frame| frame.0), Some(b'E'));
        assert_eq!(wire::error_code(error).as_deref(), Some("42883"));
        assert!(error_response_fields(error).iter().any(|(field, message)| {
            *field == 'M' && message == &format!("function width_bucket(double precision, double precision, double precision, {kind}) does not exist")
        }));
        assert_eq!(wire::error_code(&batches[index * 3 + 2]), None);
        assert_eq!(
            wire::data_rows(&batches[index * 3 + 2]),
            vec![vec![Some("1".to_owned())]]
        );
    }
}

#[test]
fn should_validate_unknown_width_bucket_count_literals_during_parse() {
    // Arrange
    let cases = [
        (
            "bad",
            "22P02",
            "invalid input syntax for type integer: \"bad\"",
        ),
        (
            "2147483648",
            "22003",
            "value \"2147483648\" is out of range for type integer",
        ),
        (
            "\u{2003}2",
            "22P02",
            "invalid input syntax for type integer: \"\u{2003}2\"",
        ),
    ];
    let mut steps = Vec::new();
    for (index, (literal, _, _)) in cases.iter().enumerate() {
        let sql = format!("SELECT WIDTH_BUCKET(NULL::float8, 0::float8, 1::float8, '{literal}')");
        steps.push(Step::ParseFlush(wire::parse_frame(
            &format!("bucket_literal_{index}"),
            &sql,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT WIDTH_BUCKET(0.5::float8, 0::float8, 1::float8, '0x2')",
        )]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-count-literal", steps);

    // Assert
    for (index, (_, state, message)) in cases.iter().enumerate() {
        let rejected = &batches[index * 3];
        assert_eq!(rejected.first().map(|frame| frame.0), Some(b'E'));
        assert_eq!(wire::error_code(rejected).as_deref(), Some(*state));
        assert!(error_response_fields(rejected)
            .iter()
            .any(|(field, value)| *field == 'M' && value == message));
        let recovered = &batches[index * 3 + 2];
        assert_eq!(wire::error_code(recovered), None);
        assert_eq!(wire::data_rows(recovered), vec![vec![Some("2".to_owned())]]);
    }
}

#[test]
fn should_reject_width_bucket_parameter_shared_by_operand_and_count() {
    // Arrange
    let cases = [
        "SELECT WIDTH_BUCKET($1, $2, $3, $1)",
        "SELECT WIDTH_BUCKET($1, $1, $2, $1)",
        "SELECT pg_catalog.width_bucket($1, $2, $3, $2)",
    ];
    let mut steps = Vec::new();
    for (index, sql) in cases.iter().enumerate() {
        steps.push(Step::ParseFlush(wire::parse_frame(
            &format!("bucket_conflict_{index}"),
            sql,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 45",
        )]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-shared-parameter", steps);

    // Assert
    for (index, _) in cases.iter().enumerate() {
        let error = &batches[index * 3];
        assert_eq!(error.first().map(|frame| frame.0), Some(b'E'));
        assert_eq!(wire::error_code(error).as_deref(), Some("42P08"));
        assert!(error_response_fields(error)
            .iter()
            .any(|(field, value)| *field == 'D' && value == "double precision versus integer"));
        assert_eq!(
            wire::data_rows(&batches[index * 3 + 2]),
            vec![vec![Some("45".to_owned())]]
        );
    }
}

#[test]
fn should_infer_width_bucket_unknown_operands_from_uniform_known_anchors() {
    // Arrange
    let cases: [(&str, &[i32], &[&str], &str); 7] = [
        (
            "SELECT WIDTH_BUCKET($1, 0::float8, 1::float8, 2)",
            &[701],
            &["0.5"],
            "2",
        ),
        (
            "SELECT WIDTH_BUCKET($1, 0::numeric, 1::numeric, 2)",
            &[1700],
            &["0.5"],
            "2",
        ),
        (
            "SELECT WIDTH_BUCKET($1, 0::numeric(2,1), 1::numeric(3,1), $2)",
            &[1700, 23],
            &["0.5", "2"],
            "2",
        ),
        (
            "SELECT WIDTH_BUCKET($1, $1, 1::numeric, 2)",
            &[1700],
            &["0"],
            "1",
        ),
        (
            "SELECT pg_catalog.width_bucket($1, 0::float8, 1::float8, 2)",
            &[701],
            &["0.5"],
            "2",
        ),
        (
            "SELECT WIDTH_BUCKET(0.5::float8, $1, 1::float8, 2)",
            &[701],
            &["0"],
            "2",
        ),
        (
            "SELECT WIDTH_BUCKET(0.5::float8, 0::float8, $1, 2)",
            &[701],
            &["1"],
            "2",
        ),
    ];
    let mut steps = Vec::new();
    for (index, (sql, _, values, _)) in cases.iter().enumerate() {
        let statement = format!("bucket_mixed_{index}");
        let portal = format!("bucket_mixed_portal_{index}");
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
    let batches = run_script("issue-813-width-bucket-mixed-inference", steps);

    // Assert
    for (index, (_, expected_types, _, expected_value)) in cases.iter().enumerate() {
        let offset = index * 4;
        assert_eq!(batches[offset].first().map(|frame| frame.0), Some(b'1'));
        let parameters = batches[offset + 1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1).as_slice(),
            *expected_types
        );
        let row = batches[offset + 1]
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("row description");
        assert_eq!(wire::parse_row_description(&row.1)[0].type_oid, 23);
        assert_eq!(wire::error_code(&batches[offset + 2]), None);
        assert_eq!(
            wire::data_rows(&batches[offset + 2]),
            vec![vec![Some((*expected_value).to_owned())]]
        );
        assert_eq!(wire::data_rows(&batches[offset + 3]), vec![vec![None]]);
    }
}

#[test]
fn should_preserve_declared_width_bucket_routine_parameter_inference_with_known_anchors() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            r#"CREATE FUNCTION width_bucket(a TEXT, b TEXT, c TEXT, d TEXT) RETURNS TEXT AS "d""#,
        )]),
        Step::ParseFlush(wire::parse_frame(
            "bucket_mixed_declared",
            "SELECT WIDTH_BUCKET($1, 0::float8, 1::float8, 2)",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_mixed_declared"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-813-width-bucket-mixed-routine", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let parameters = batches[2]
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [25]);
}

#[test]
fn should_report_mixed_width_bucket_count_cast_conflicts_during_parse() {
    // Arrange
    let cases = [
        "SELECT WIDTH_BUCKET($1, 0::float8, 1::float8, $1::int4)",
        "SELECT WIDTH_BUCKET($1, 0::float8, 1::float8, $1::int4 + 1)",
    ];
    let mut steps = Vec::new();
    for (index, sql) in cases.iter().enumerate() {
        steps.push(Step::ParseFlush(wire::parse_frame(
            &format!("bucket_mixed_conflict_{index}"),
            sql,
        )));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 45",
        )]));
    }

    // Act
    let batches = run_script("issue-813-width-bucket-mixed-count-conflict", steps);

    // Assert
    for index in 0..cases.len() {
        let error = &batches[index * 3];
        assert_eq!(wire::error_code(error).as_deref(), Some("42P08"));
        assert!(error_response_fields(error)
            .iter()
            .any(|(field, value)| *field == 'D' && value == "integer versus double precision"));
        assert_eq!(
            wire::data_rows(&batches[index * 3 + 2]),
            vec![vec![Some("45".to_owned())]]
        );
    }
}

#[test]
fn should_infer_unknown_width_bucket_operands_with_a_known_int4_count() {
    // Arrange
    let cases: [(&str, &[i32], &[&str], &str); 2] = [
        (
            "SELECT WIDTH_BUCKET($1, $2, $3, 2)",
            &[701, 701, 701],
            &["0.5", "0", "1"],
            "2",
        ),
        (
            "SELECT WIDTH_BUCKET($1, $1, $2, 2::int4)",
            &[701, 701],
            &["0", "1"],
            "1",
        ),
    ];
    let mut steps = Vec::new();
    for (index, (sql, _, values, _)) in cases.iter().enumerate() {
        let statement = format!("bucket_known_count_{index}");
        let portal = format!("bucket_known_count_portal_{index}");
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
    }
    steps.push(Step::ParseFlush(wire::parse_frame(
        "bucket_known_count_conflict",
        "SELECT WIDTH_BUCKET($1, $2, $3, $1::int4)",
    )));
    steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 45",
    )]));

    // Act
    let batches = run_script("issue-813-width-bucket-known-count", steps);

    // Assert
    for (index, (_, expected_types, _, expected_value)) in cases.iter().enumerate() {
        let offset = index * 3;
        assert_eq!(batches[offset].first().map(|frame| frame.0), Some(b'1'));
        let parameters = batches[offset + 1]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1).as_slice(),
            *expected_types
        );
        assert_eq!(wire::error_code(&batches[offset + 2]), None);
        assert_eq!(
            wire::data_rows(&batches[offset + 2]),
            vec![vec![Some((*expected_value).to_owned())]]
        );
    }
    assert_eq!(wire::error_code(&batches[6]).as_deref(), Some("42P08"));
    assert!(error_response_fields(&batches[6])
        .iter()
        .any(|(field, value)| *field == 'D' && value == "integer versus double precision"));
    assert_eq!(
        wire::data_rows(&batches[8]),
        vec![vec![Some("45".to_owned())]]
    );
}

#[test]
fn should_preserve_known_count_null_later_cast_and_declared_routine_paths() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "bucket_known_later",
            "SELECT WIDTH_BUCKET($1, $2, $3, 2), $1::text",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_known_later"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bucket_known_later",
                &[0],
                &[None, Some(b"0"), Some(b"1")],
                &[],
            ),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            r#"CREATE FUNCTION width_bucket(a TEXT, b TEXT, c TEXT, d TEXT) RETURNS TEXT AS "d""#,
        )]),
        Step::ParseFlush(wire::parse_frame(
            "bucket_known_routine",
            "SELECT WIDTH_BUCKET($1, $2, $3, 2)",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("bucket_known_routine"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-813-width-bucket-known-count-context", steps);

    // Assert
    for (index, expected) in [(1, [701, 701, 701]), (5, [25, 25, 25])] {
        let parameters = batches[index]
            .iter()
            .find(|frame| frame.0 == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), expected);
    }
    assert_eq!(wire::data_rows(&batches[2]), vec![vec![None, None]]);
    assert_eq!(wire::error_code(&batches[3]), None);
}
