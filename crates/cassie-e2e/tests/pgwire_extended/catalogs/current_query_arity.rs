//! Native zero-argument signatures reject extra inputs before output metadata.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_extra_argument_0_given_invalid_signature_when_current_query_is_parsed() {
    // Arrange
    let sql = "SELECT current_query(1)";
    // Act
    let batches = run_script(
        "query-arity-0",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT current_query() AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0]
            .iter()
            .filter(|frame| matches!(frame.0, b'T' | b'D'))
            .count(),
        0
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("SELECT current_query() AS output".to_owned())]]
    );
}

#[test]
fn should_reject_extra_argument_1_given_invalid_signature_when_current_query_is_parsed() {
    // Arrange
    let sql = "SELECT pg_catalog.current_query(NULL)";
    // Act
    let batches = run_script(
        "query-arity-1",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT current_query() AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0]
            .iter()
            .filter(|frame| matches!(frame.0, b'T' | b'D'))
            .count(),
        0
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("SELECT current_query() AS output".to_owned())]]
    );
}

#[test]
fn should_reject_extra_argument_2_given_invalid_signature_when_current_query_is_parsed() {
    // Arrange
    let sql = "SELECT current_query($1::TEXT)";
    // Act
    let batches = run_script(
        "query-arity-2",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT current_query() AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0]
            .iter()
            .filter(|frame| matches!(frame.0, b'T' | b'D'))
            .count(),
        0
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("SELECT current_query() AS output".to_owned())]]
    );
}

#[test]
fn should_reject_extra_argument_3_given_invalid_signature_when_current_query_is_parsed() {
    // Arrange
    let sql = "SELECT pg_catalog.current_query(1,2)";
    // Act
    let batches = run_script(
        "query-arity-3",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT current_query() AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        batches[0]
            .iter()
            .filter(|frame| matches!(frame.0, b'T' | b'D'))
            .count(),
        0
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("SELECT current_query() AS output".to_owned())]]
    );
}
