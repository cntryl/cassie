//! PostgreSQL 18.6 residual scalar-function identity and rounding contracts.

use super::*;

#[test]
fn should_report_postgresql_math_result_oids_across_function_overloads() {
    // Arrange
    let sql = "SELECT \
        ROUND(1.25::numeric, 1), \
        ROUND(2.5::float8), \
        TRUNC(42.438::numeric, 2), \
        FLOOR(42.8::numeric), FLOOR(42.8::float8), \
        CEILING(42.8::numeric), CEILING(42.8::float8), \
        MOD(9::int4, 4::int4), MOD(9::numeric, 4::numeric), \
        DIV(9::int4, 4::int4), DIV(9::numeric, 4::numeric), \
        POWER(2::numeric, 3::numeric), POWER(2::float8, 3::float8), \
        SQRT(4::numeric), SQRT(4::float8), CBRT(-8::numeric), \
        EXP(1::numeric), LN(2::numeric), LOG(100::numeric), LOG10(100::numeric), \
        SIGN(-1::numeric), SIGN(-1::float8), PI(), DEGREES(PI()), RADIANS(180), \
        SIN(1::numeric), SIND(30), SINH(1::numeric), ERF(1::numeric), \
        GAMMA(6::numeric), LGAMMA(1::numeric), FACTORIAL(5::int4), \
        GCD(12::int4, 8::int4), LCM(12::int4, 8::int4), \
        SCALE(8.4100::numeric), MIN_SCALE(8.4100::numeric), \
        TRIM_SCALE(8.4100::numeric), \
        WIDTH_BUCKET(2::numeric, 0::numeric, 5::numeric, 5), \
        WIDTH_BUCKET(2::float8, 0::float8, 5::float8, 5), \
        WIDTH_BUCKET(2, ARRAY[1, 3, 7]), \
        RANDOM_NORMAL(4.5, 0.0), RANDOM(1, 10), RANDOM()";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-math-result-identities", steps);

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
        .expect("math result RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![
            1700, 701, 1700, 1700, 701, 1700, 701, 23, 1700, 1700, 1700, 1700, 701, 1700, 701, 701,
            1700, 1700, 1700, 1700, 1700, 701, 701, 701, 701, 701, 701, 701, 701, 701, 701, 1700,
            23, 23, 23, 23, 1700, 23, 23, 23, 701, 23, 701,
        ]
    );
    assert_eq!(wire::data_rows(frames).len(), 1);
    assert_eq!(wire::data_rows(frames)[0].len(), 43);
}

#[test]
fn should_round_float8_midpoints_to_even() {
    // Arrange
    let sql = "SELECT \
        ROUND(2.5::numeric), ROUND(-2.5::numeric), \
        ROUND(2.5::float8), ROUND(3.5::float8), \
        ROUND(-2.5::float8), ROUND(-3.5::float8)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-rounding-tie-distinction", steps);

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames), None);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("rounding tie RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700, 701, 701, 701, 701]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("3".to_owned()),
            Some("-3".to_owned()),
            Some("2".to_owned()),
            Some("4".to_owned()),
            Some("-2".to_owned()),
            Some("-4".to_owned()),
        ]]
    );
}

#[test]
fn should_reject_float8_round_with_a_scale_argument() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROUND(1.5::float8, 1)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-round-float8-scale-error", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert!(!batches[0]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_reject_float8_trunc_with_a_scale_argument() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT TRUNC(1.5::float8, 1)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42 AS recovery")]),
    ];

    // Act
    let batches = run_script("postgresql18-trunc-float8-scale-error", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert!(!batches[0]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_report_division_by_zero_from_div() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT DIV(9::int4, 0::int4)",
    )])];

    // Act
    let batches = run_script("postgresql18-div-zero-sqlstate", steps);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22012"));
    assert!(!batches[0]
        .iter()
        .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')));
}
