//! Exact NUMERIC power obligations, formerly staged until the carrier existed.
//!
//! PostgreSQL 18.6 evaluates decimal-literal and NUMERIC powers with the
//! numeric overload (oracle-corrections.md).
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn run_power(label: &str, sql: &str) -> Vec<Vec<(u8, Vec<u8>)>> {
    run_script(
        label,
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", "p", &[], &[], &[]),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 2 ^ 3 AS output")]),
        ],
    )
}

fn assert_numeric_power(batches: &[Vec<(u8, Vec<u8>)>], sql: &str, expected: &str) {
    assert_eq!(wire::error_code(&batches[0]), None, "{sql}");
    fixture::assert_success(&batches[1]);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric power RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1700
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some(expected.to_owned())]],
        "{sql}"
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("8".to_owned())]]
    );
}

#[test]
fn should_evaluate_decimal_literal_power_operator_as_numeric() {
    // Arrange
    let sql = "SELECT 1.5 ^ 2 AS output";

    // Act
    let batches = run_power("power-numeric-0", sql);

    // Assert
    assert_numeric_power(&batches, sql, "2.2500000000000000");
}

#[test]
fn should_evaluate_decimal_literal_power_function_as_numeric() {
    // Arrange
    let sql = "SELECT power(1.5,2) AS output";

    // Act
    let batches = run_power("power-numeric-1", sql);

    // Assert
    assert_numeric_power(&batches, sql, "2.2500000000000000");
}

#[test]
fn should_evaluate_decimal_expression_power_function_as_numeric() {
    // Arrange
    let sql = "SELECT power(1.5+2.5,2) AS output";

    // Act
    let batches = run_power("power-numeric-2", sql);

    // Assert
    assert_numeric_power(&batches, sql, "16.000000000000000");
}

#[test]
fn should_evaluate_numeric_cast_power_function_as_numeric() {
    // Arrange
    let sql = "SELECT power(1::NUMERIC,2) AS output";

    // Act
    let batches = run_power("power-numeric-3", sql);

    // Assert
    assert_numeric_power(&batches, sql, "1.0000000000000000");
}

#[test]
fn should_resolve_unknown_power_operand_to_numeric_when_the_other_is_numeric() {
    // Arrange
    let cases = [
        "SELECT '2' ^ 3::numeric AS output",
        "SELECT power('2', 3::numeric) AS output",
        "SELECT pg_catalog.power(2::numeric, 3) AS output",
    ];

    // Act
    let batches = cases.map(|sql| run_power("power-numeric-unknown", sql));

    // Assert
    for (batch, sql) in batches.iter().zip(cases) {
        assert_numeric_power(batch, sql, "8.0000000000000000");
    }
}

#[test]
fn should_resolve_mixed_numeric_and_float_power_to_float8() {
    // Arrange
    let sql = "SELECT power(2::numeric, 3::float8) AS output";

    // Act
    let batches = run_power("power-numeric-float", sql);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    fixture::assert_success(&batches[1]);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("mixed power RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 701);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("8".to_owned())]]
    );
    fixture::assert_success(&batches[2]);
}

#[test]
fn should_infer_power_parameter_from_the_selected_overload() {
    // Arrange
    let cases = [
        ("SELECT $1 ^ 3::numeric", 1700, 1700, "8.0000000000000000"),
        (
            "SELECT power($1, 3::numeric)",
            1700,
            1700,
            "8.0000000000000000",
        ),
        ("SELECT $1 ^ 3::float8", 701, 701, "8"),
        ("SELECT power($1, 3::float8)", 701, 701, "8"),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _, _)| fixture::execute_cycle(sql, Some((0, 0, Some(b"2"))), 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("power-parameter-overloads", &[], cycles);

    // Assert
    for (frames, (_, parameter_oid, result_oid, result)) in batches.iter().zip(cases) {
        fixture::assert_success(frames);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameters");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            vec![parameter_oid]
        );
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("row description");
        assert_eq!(
            wire::parse_row_description(&description.1)[0].type_oid,
            result_oid
        );
        assert_eq!(wire::data_rows(frames), vec![vec![Some(result.to_owned())]]);
    }
}

#[test]
fn should_reject_numeric_to_float_overflow_and_underflow_for_mixed_power() {
    // Arrange
    let cases = [
        "SELECT power(1e1000::numeric, 2::float8)",
        "SELECT power(1e-1000::numeric, 2::float8)",
        "SELECT 1e1000::numeric ^ 2::float8",
        "SELECT 1e-1000::numeric ^ 2::float8",
    ];
    let cycles = cases.map(|sql| fixture::execute_cycle(sql, None, 0, true));

    // Act
    let batches = fixture::run_cycles("power-numeric-float-range", &[], cycles.into());

    // Assert
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("22003"));
    }
}

#[test]
fn should_reject_typed_text_power_arguments_while_accepting_unknown_literals() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT power('2'::text, 3)", None, 0, true),
        fixture::execute_cycle("SELECT '2'::text ^ 3::numeric", None, 0, true),
        fixture::execute_cycle(
            "SELECT power(value, 3) FROM power_text_values",
            None,
            0,
            true,
        ),
        fixture::execute_cycle("SELECT power($1, 3)", Some((25, 0, Some(b"2"))), 0, true),
    ];
    let setup = [("CREATE TABLE power_text_values (value TEXT)", vec![])];

    // Act
    let batches = fixture::run_cycles("power-typed-text", &setup, cycles);

    // Assert
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("42883"));
    }
}

#[test]
fn should_coerce_invalid_unknown_power_literals_before_strict_null_result() {
    // Arrange
    let cases = [
        "SELECT power('bad', NULL::numeric)",
        "SELECT 'bad' ^ NULL::numeric",
        "SELECT NULL::numeric ^ 'bad'",
        "SELECT power('bad', NULL::float8)",
    ];
    let cycles = cases.map(|sql| fixture::execute_cycle(sql, None, 0, true));

    // Act
    let batches = fixture::run_cycles("power-unknown-null", &[], cycles.into());

    // Assert
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("22P02"));
    }
}

#[test]
fn should_coerce_invalid_unknown_power_literal_before_null_parameter() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT power('bad', $1::numeric)",
        Some((1700, 0, None)),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("power-unknown-null-parameter", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}

#[test]
fn should_reject_invalid_power_literal_during_parse_even_in_unreachable_case() {
    // Arrange
    let cases = [
        "SELECT power('bad', $1::numeric)",
        "SELECT CASE WHEN false THEN power('bad', $1::numeric) ELSE 1::numeric END",
    ];

    // Act
    let batches = cases.map(|sql| {
        run_script(
            "power-invalid-literal-parse",
            vec![
                Step::ParseFlush(wire::parse_frame("p", sql)),
                Step::ThroughReady(vec![wire::sync_frame()]),
                Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
            ],
        )
    });

    // Assert
    for response in batches {
        assert_eq!(wire::error_code(&response[0]).as_deref(), Some("22P02"));
        fixture::assert_success(&response[2]);
    }
}

#[test]
fn should_preserve_user_power_routine_precedence_over_builtin_literal_coercion() {
    // Arrange
    let setup = [(
        "CREATE FUNCTION power(x TEXT, y INT) RETURNS INT AS \"99\"",
        vec![],
    )];
    let cycle = fixture::execute_cycle("SELECT power('bad'::text, 3)", None, 0, true);

    // Act
    let batches = fixture::run_cycles("power-user-routine", &setup, vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("99".to_owned())]]
    );
}
