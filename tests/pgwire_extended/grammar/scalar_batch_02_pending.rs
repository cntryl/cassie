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
