//! PostgreSQL 18 collection aggregate array-input behavior tracked by #811.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

fn array_aggregate_setup() -> [(&'static str, Vec<Value>); 2] {
    [
        (
            "CREATE TABLE issue_811_array_aggregate_input (ordinal INT4 PRIMARY KEY, item INT4[])",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_array_aggregate_input VALUES (1, $$[0:1]={1,2}$$::INT4[]), (2, $$[0:1]={3,4}$$::INT4[])",
            Vec::new(),
        ),
    ]
}

fn int4_array_element(value: i32, output: &mut Vec<u8>) {
    output.extend_from_slice(&4_i32.to_be_bytes());
    output.extend_from_slice(&value.to_be_bytes());
}

fn nested_array_binary_value() -> Vec<u8> {
    let mut value = Vec::new();
    value.extend_from_slice(&2_i32.to_be_bytes()); // dimensions
    value.extend_from_slice(&0_i32.to_be_bytes()); // no NULL elements
    value.extend_from_slice(&23_i32.to_be_bytes()); // INT4 element OID
    value.extend_from_slice(&2_i32.to_be_bytes()); // outer length
    value.extend_from_slice(&1_i32.to_be_bytes()); // outer lower bound
    value.extend_from_slice(&2_i32.to_be_bytes()); // inner length
    value.extend_from_slice(&0_i32.to_be_bytes()); // preserved inner lower bound
    for element in [1, 2, 3, 4] {
        int4_array_element(element, &mut value);
    }
    value
}

#[test]
fn should_aggregate_array_inputs_into_a_higher_dimension_in_text_wire() {
    // Arrange
    let sql = "SELECT array_agg(item ORDER BY ordinal) FROM issue_811_array_aggregate_input";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-811-array-aggregate-higher-dimension",
        &array_aggregate_setup(),
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1:2][0:1]={{1,2},{3,4}}".to_owned())]]
    );
}

#[test]
fn should_aggregate_array_inputs_into_a_higher_dimension_in_binary_wire() {
    // Arrange
    let sql = "SELECT array_agg(item ORDER BY ordinal) FROM issue_811_array_aggregate_input";
    let cycle = fixture::execute_cycle(sql, None, 1, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-811-array-aggregate-higher-dimension-binary",
        &array_aggregate_setup(),
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1_007, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(&nested_array_binary_value())]
    );
}

fn run_array_aggregate_error_case(label: &str, sql: &str) -> Vec<fixture::Frames> {
    fixture::run_cycles(
        label,
        &array_aggregate_setup(),
        vec![
            fixture::execute_cycle(sql, None, 0, true),
            fixture::execute_cycle("SELECT 7::INT4", None, 0, true),
        ],
    )
}

fn assert_array_aggregate_error_and_recovery(
    batches: &[fixture::Frames],
    sql: &str,
    expected_sqlstate: &str,
) {
    let error = &batches[0];
    assert_eq!(
        wire::error_code(error).as_deref(),
        Some(expected_sqlstate),
        "wrong PostgreSQL SQLSTATE for {sql}"
    );
    assert!(wire::data_rows(error).is_empty());
    assert_eq!(error.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    assert_eq!(error.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(
        error.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_a_null_array_aggregate_input_with_22004() {
    // Arrange
    let sql = "SELECT array_agg(item) FROM (VALUES (NULL::INT4[])) AS input(item)";

    // Act
    let batches = run_array_aggregate_error_case("issue-811-array-aggregate-null-input", sql);

    // Assert
    assert_array_aggregate_error_and_recovery(&batches, sql, "22004");
}

#[test]
fn should_reject_an_empty_array_aggregate_input_with_2202e() {
    // Arrange
    let sql = "SELECT array_agg(item) FROM (VALUES (ARRAY[]::INT4[])) AS input(item)";

    // Act
    let batches = run_array_aggregate_error_case("issue-811-array-aggregate-empty-input", sql);

    // Assert
    assert_array_aggregate_error_and_recovery(&batches, sql, "2202E");
}

#[test]
fn should_reject_differently_shaped_array_aggregate_inputs_with_2202e() {
    // Arrange
    let sql = "SELECT array_agg(item) FROM (VALUES (ARRAY[1,2]::INT4[]), (ARRAY[[3,4],[5,6]]::INT4[])) AS input(item)";

    // Act
    let batches =
        run_array_aggregate_error_case("issue-811-array-aggregate-different-dimensions", sql);

    // Assert
    assert_array_aggregate_error_and_recovery(&batches, sql, "2202E");
}
