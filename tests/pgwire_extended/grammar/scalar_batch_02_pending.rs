//! Native-valid NUMERIC obligations reject explicitly until their carrier is implemented.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_stage_numeric_0_given_unimplemented_identity_when_power_is_parsed() {
    // Arrange
    let sql = "SELECT 1.5 ^ 2 AS output";
    // Act
    let batches = run_script(
        "power-pending-0",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 2 ^ 3 AS output")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("8".to_owned())]]
    );
}

#[test]
fn should_stage_numeric_1_given_unimplemented_identity_when_power_is_parsed() {
    // Arrange
    let sql = "SELECT power(1.5,2) AS output";
    // Act
    let batches = run_script(
        "power-pending-1",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 2 ^ 3 AS output")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("8".to_owned())]]
    );
}

#[test]
fn should_stage_numeric_2_given_unimplemented_identity_when_power_is_parsed() {
    // Arrange
    let sql = "SELECT power(1.5+2.5,2) AS output";
    // Act
    let batches = run_script(
        "power-pending-2",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 2 ^ 3 AS output")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("8".to_owned())]]
    );
}

#[test]
fn should_stage_numeric_3_given_unimplemented_identity_when_power_is_parsed() {
    // Arrange
    let sql = "SELECT power(1::NUMERIC,2) AS output";
    // Act
    let batches = run_script(
        "power-pending-3",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 2 ^ 3 AS output")]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("8".to_owned())]]
    );
}
