//! FLOAT8 power range diagnostics and recovery, pinned to PostgreSQL 18.6.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_distinguish_float_power_range_errors_by_call_form() {
    // Arrange
    let cases = [
        ("SELECT power(1e308::float8, 2::float8)", "overflow"),
        ("SELECT power(1e-300::float8, 2::float8)", "underflow"),
        ("SELECT 1e308::float8 ^ 2::float8", "overflow"),
        ("SELECT 1e-300::float8 ^ 2::float8", "underflow"),
    ];
    let mut steps = Vec::new();
    for (sql, _) in cases {
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(sql)]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42",
        )]));
    }

    // Act
    let batches = run_script("float-power-range-messages", steps);

    // Assert
    for ((_, expected), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some("22003"));
        let error = pair[0]
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("range ErrorResponse");
        assert!(
            wire::parse_error_fields(&error.1)
                .iter()
                .any(|(kind, text)| *kind == 'M'
                    && text == &format!("value out of range: {expected}"))
        );
        assert!(!pair[0].iter().any(|(tag, _)| matches!(*tag, b'D' | b'C')));
        assert_eq!(wire::error_code(&pair[1]), None);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("42".to_owned())]]);
    }
}

#[test]
fn should_report_bound_float_power_overflow_after_sync() {
    // Arrange
    let statement = "float_power_range";
    let portal = "float_power_range_portal";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT power($1, $2)",
            &[701, 701],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["1e308", "2"]),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("bound-float-power-range", steps);

    // Assert
    let parameters = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [701, 701]);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("result description");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 701);
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22003"));
    let error = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .expect("range ErrorResponse");
    assert!(wire::parse_error_fields(&error.1)
        .iter()
        .any(|(kind, text)| *kind == 'M' && text == "value out of range: overflow"));
    assert_eq!(wire::error_code(&batches[3]), None);
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_preserve_float_power_special_cases() {
    // Arrange
    let sql = "SELECT power('NaN'::float8, 0::float8), \
        power(1::float8, 'NaN'::float8), \
        power('-Infinity'::float8, -3::float8), \
        power(NULL::float8, 2::float8)";

    // Act
    let batches = run_script(
        "float-power-special-controls",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("1".into()),
            Some("1".into()),
            Some("-0".into()),
            None
        ]]
    );
}

#[test]
fn should_report_folded_power_range_at_bind() {
    // Arrange
    let cases = [
        (
            "power_folded_overflow",
            "SELECT CASE WHEN TRUE THEN power(1e308::float8,2::float8) ELSE 1::float8 END",
            "overflow",
        ),
        (
            "power_folded_underflow",
            "SELECT COALESCE(NULL::float8, 1e-300::float8 ^ 2::float8)",
            "underflow",
        ),
    ];
    let mut steps = Vec::new();
    for (statement, sql, _) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[],
        )));
        steps.push(Step::BindFlush(wire::bind_frame("", statement, &[])));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 42",
        )]));
    }

    // Act
    let batches = run_script("folded-power-range-bind", steps);

    // Assert
    for ((_, _, expected), cycle) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        assert_eq!(cycle[0][0].0, b'1');
        assert_eq!(wire::error_code(&cycle[1]).as_deref(), Some("22003"));
        let error = cycle[1]
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("bind error");
        assert!(
            wire::parse_error_fields(&error.1)
                .iter()
                .any(|(kind, text)| *kind == 'M'
                    && text == &format!("value out of range: {expected}"))
        );
        assert!(!cycle[1]
            .iter()
            .any(|(tag, _)| matches!(*tag, b'2' | b'D' | b'C')));
        assert_eq!(wire::error_code(&cycle[3]), None);
        assert_eq!(wire::data_rows(&cycle[3]), vec![vec![Some("42".into())]]);
    }
}
