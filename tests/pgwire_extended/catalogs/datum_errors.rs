//! Native signature errors and explicit staged layout boundaries.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_signature_0_given_invalid_inputs_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size()";
    // Act
    let batches = run_script(
        "datum-signature-0",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size(0::INT) AS output",
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
        vec![vec![Some("4".to_owned())]]
    );
}

#[test]
fn should_reject_signature_1_given_invalid_inputs_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_catalog.pg_column_size(1,2)";
    // Act
    let batches = run_script(
        "datum-signature-1",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size(0::INT) AS output",
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
        vec![vec![Some("4".to_owned())]]
    );
}

#[test]
fn should_reject_signature_2_given_invalid_inputs_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size($1)";
    // Act
    let batches = run_script(
        "datum-signature-2",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size(0::INT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42P18"));
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
        vec![vec![Some("4".to_owned())]]
    );
}

#[test]
fn should_reject_signature_3_given_invalid_inputs_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(DISTINCT 1)";
    // Act
    let batches = run_script(
        "datum-signature-3",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size(0::INT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42809"));
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
        vec![vec![Some("4".to_owned())]]
    );
}

#[test]
fn should_reject_signature_4_given_invalid_inputs_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(1 ORDER BY 1)";
    // Act
    let batches = run_script(
        "datum-signature-4",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size(0::INT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42809"));
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
        vec![vec![Some("4".to_owned())]]
    );
}

#[test]
fn should_reject_signature_5_given_invalid_inputs_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(1) FILTER (WHERE TRUE)";
    // Act
    let batches = run_script(
        "datum-signature-5",
        vec![
            Step::ParseFlush(wire::parse_frame("bad", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size(0::INT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42809"));
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
        vec![vec![Some("4".to_owned())]]
    );
}

#[test]
fn should_reject_layout_0_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(1::NUMERIC)";
    // Act
    let batches = run_script(
        "datum-layout-0",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_layout_1_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(1.0)";
    // Act
    let batches = run_script(
        "datum-layout-1",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_layout_2_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size('x'::CHAR(4))";
    // Act
    let batches = run_script(
        "datum-layout-2",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_layout_3_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(NULL::CHAR(4))";
    // Act
    let batches = run_script(
        "datum-layout-3",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_layout_4_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size('{}'::JSON)";
    // Act
    let batches = run_script(
        "datum-layout-4",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_layout_5_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(ARRAY['a']::TEXT[])";
    // Act
    let batches = run_script(
        "datum-layout-5",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_layout_6_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size(NULL::TEXT[])";
    // Act
    let batches = run_script(
        "datum-layout-6",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_layout_7_given_pending_provenance_when_column_size_is_parsed() {
    // Arrange
    let sql = "WITH q AS (SELECT 'foo'::TEXT AS v) SELECT pg_column_size(v::TEXT) FROM q";
    // Act
    let batches = run_script(
        "datum-layout-7",
        vec![
            Step::ParseFlush(wire::parse_frame("p", sql)),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size('foo'::TEXT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_direct_typed_parameters_given_unpropagated_identity_when_column_size_is_parsed() {
    // Arrange
    let sql = "SELECT pg_column_size($1)";
    // Act
    let batches = run_script(
        "datum-direct-param",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types("p", sql, &[21])),
            Step::ThroughReady(vec![wire::sync_frame()]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT pg_column_size(1::SMALLINT) AS output",
            )]),
        ],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("0A000"));
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
        vec![vec![Some("2".to_owned())]]
    );
}
