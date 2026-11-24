//! Native truth input diagnostics and recovery before result metadata.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_reject_truth_input_0_given_wrong_boolean_type_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 1 IS TRUE AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("truth-error-0", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42804",
        "argument of IS TRUE must be type boolean, not type integer",
    );
}

#[test]
fn should_reject_truth_input_1_given_wrong_boolean_type_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 1 IS NOT TRUE AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("truth-error-1", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42804",
        "argument of IS NOT TRUE must be type boolean, not type integer",
    );
}

#[test]
fn should_reject_truth_input_2_given_wrong_boolean_type_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'yes'::TEXT IS UNKNOWN AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("truth-error-2", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42804",
        "argument of IS UNKNOWN must be type boolean, not type text",
    );
}

#[test]
fn should_reject_truth_input_3_given_wrong_boolean_type_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT 'wat' IS TRUE AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, None, format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("truth-error-3", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "22P02",
        "invalid input syntax for type boolean: \"wat\"",
    );
}

#[test]
fn should_reject_truth_input_4_given_wrong_boolean_type_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT $1 IS TRUE AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((23, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("truth-error-4", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42804",
        "argument of IS TRUE must be type boolean, not type integer",
    );
}

#[test]
fn should_reject_truth_input_5_given_wrong_boolean_type_when_native_diagnostic_is_observed() {
    // Arrange
    let sql = "SELECT $1 IS NOT UNKNOWN AS output";
    let cycles = [0, 1]
        .map(|format| fixture::execute_cycle(sql, Some((25, 0, None)), format, true))
        .to_vec();
    // Act
    let batches = fixture::run_cycles("truth-error-5", &[], cycles);
    // Assert
    assert_errors(
        &batches,
        "42804",
        "argument of IS NOT UNKNOWN must be type boolean, not type text",
    );
}

fn assert_errors(batches: &[fixture::Frames], state: &str, message: &str) {
    for frames in batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some(state));
        assert!(!frames
            .iter()
            .any(|(tag, _)| matches!(tag, b'T' | b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        let body = &frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("error")
            .1;
        let actual = body
            .split(|byte| *byte == 0)
            .find_map(|part| part.strip_prefix(b"M"))
            .expect("message");
        assert_eq!(actual, message.as_bytes());
    }
}
