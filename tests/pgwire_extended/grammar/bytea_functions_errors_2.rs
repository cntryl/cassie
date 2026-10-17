//! Native fixed BYTEA error timing and diagnostics.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_error_30_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_bit('\\x00'::BYTEA,9223372036854775807) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-30", steps);
    // Assert
    let frames = &batches[1];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("2202E"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("index 9223372036854775807 out of valid range, 0..7"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_31_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_bit('\\x00'::BYTEA,-9223372036854775808) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-31", steps);
    // Assert
    let frames = &batches[1];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("2202E"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("index -9223372036854775808 out of valid range, 0..7"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_32_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT set_bit('\\x00'::BYTEA,9223372036854775807,1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-32", steps);
    // Assert
    let frames = &batches[1];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("2202E"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("index 9223372036854775807 out of valid range, 0..7"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_33_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT lower(decode('00','hex')) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-33", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("42883"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function lower(bytea) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_34_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT encode('x'::TEXT,'hex'::TEXT) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-34", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("42883"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function encode(text, text) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_35_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT encode('\\q',1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-35", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("42883"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function encode(unknown, integer) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_36_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT encode('\\q'::BYTEA,1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-36", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22P02"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("invalid input syntax for type bytea"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_37_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT encode('bad'::INT,1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-37", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22P02"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("invalid input syntax for type integer: \"bad\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_38_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT get_byte('\\q',FALSE) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-38", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("42883"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("function get_byte(unknown, boolean) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_39_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT get_byte('\\q','bad') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-39", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22P02"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("invalid input syntax for type bytea"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_40_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT decode('00','bad'::INT) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-40", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22P02"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("invalid input syntax for type integer: \"bad\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}
