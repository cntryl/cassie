//! Unknown argument overload selection must not invent a BYTEA signature.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_ambiguous_signature_3_given_unknown_bit_inputs_when_parse_selects_overloads() {
    // Arrange
    let sql = "SELECT bit_count('abc')";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bytea-unknown-overload-3", steps);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("42725"),
        "{batches:?}"
    );
    let body = &batches[0].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function bit_count(unknown) is not unique")
    );
}

#[test]
fn should_preserve_ambiguous_signature_4_given_unknown_bit_inputs_when_parse_selects_overloads() {
    // Arrange
    let sql = "SELECT bit_count(NULL)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bytea-unknown-overload-4", steps);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("42725"),
        "{batches:?}"
    );
    let body = &batches[0].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function bit_count(unknown) is not unique")
    );
}

#[test]
fn should_preserve_ambiguous_signature_5_given_unknown_bit_inputs_when_parse_selects_overloads() {
    // Arrange
    let sql = "SELECT bit_count($1)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bytea-unknown-overload-5", steps);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("42725"),
        "{batches:?}"
    );
    let body = &batches[0].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function bit_count(unknown) is not unique")
    );
}

#[test]
fn should_preserve_ambiguous_signature_7_given_unknown_bit_inputs_when_parse_selects_overloads() {
    // Arrange
    let sql = "SELECT set_bit($1,$2,$3)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bytea-unknown-overload-7", steps);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("42725"),
        "{batches:?}"
    );
    let body = &batches[0].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function set_bit(unknown, unknown, unknown) is not unique")
    );
}

#[test]
fn should_preserve_ambiguous_signature_8_given_unknown_bit_inputs_when_parse_selects_overloads() {
    // Arrange
    let sql = "SELECT get_bit($1,$2)";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bytea-unknown-overload-8", steps);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("42725"),
        "{batches:?}"
    );
    let body = &batches[0].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function get_bit(unknown, unknown) is not unique")
    );
}

#[test]
fn should_preserve_ambiguous_signature_12_given_unknown_bit_inputs_when_parse_selects_overloads() {
    // Arrange
    let sql = "SELECT get_bit('abc','0')";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bytea-unknown-overload-12", steps);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("42725"),
        "{batches:?}"
    );
    let body = &batches[0].iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function get_bit(unknown, unknown) is not unique")
    );
}

#[test]
fn should_keep_bit_carrier_pending_given_unknown_first_argument_when_int4_index_selects_bit() {
    // Arrange
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", "SELECT get_bit($1,0)")),
        Step::ThroughReady(vec![wire::sync_frame()]),
    ];
    // Act
    let batches = run_script("bytea-unknown-pending-bit", steps);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("0A000"),
        "{batches:?}"
    );
}
