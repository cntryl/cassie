//! Numeric text adapters retain their input origin without adopting a result ABI.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

fn assert_parameter_oid(frames: &fixture::Frames, expected: i32) {
    let (_, payload) = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(payload), vec![expected]);
}

#[test]
fn should_return_numeric_result_for_sum_numeric_parameter() {
    // Arrange
    let sql = "SELECT SUM($1) AS v";
    let cycles = vec![fixture::execute_cycle(
        sql,
        Some((1700, 0, Some(b"2.5"))),
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("type-numeric-sum-parameter-result", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1700, -1, -1), 0, true);
    assert_parameter_oid(frames, 1700);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"2.5")]
    );
}

#[test]
fn should_return_real_result_for_sum_real_parameter() {
    // Arrange
    let sql = "SELECT SUM($1) AS v";
    let cycles = vec![fixture::execute_cycle(
        sql,
        Some((700, 0, Some(b"2.5"))),
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("type-real-sum-parameter-result", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (700, 4, -1), 0, true);
    assert_parameter_oid(frames, 700);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"2.5")]
    );
}

#[test]
fn should_return_numeric_result_for_avg_numeric_parameter() {
    // Arrange
    let sql = "SELECT AVG($1) AS v";
    let cycles = vec![fixture::execute_cycle(
        sql,
        Some((1700, 0, Some(b"9007199254740993"))),
        0,
        true,
    )];

    // Act
    let batches = fixture::run_cycles("type-numeric-avg-parameter-result", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (1700, -1, -1), 0, true);
    assert_parameter_oid(frames, 1700);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"9007199254740993.0000")]
    );
}

include!("../../types/numeric/numeric_metadata.rs");
