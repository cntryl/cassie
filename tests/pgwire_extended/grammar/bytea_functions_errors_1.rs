//! Native fixed BYTEA error timing and diagnostics.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_error_15_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_bit('\\x00'::BYTEA,8) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-15", steps);
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
        Some("index 8 out of valid range, 0..7"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_16_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_bit('\\x'::BYTEA,0) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-16", steps);
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
        Some("index 0 out of valid range, 0..-1"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_17_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT set_byte('\\x00'::BYTEA,2,256) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-17", steps);
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
        Some("index 2 out of valid range, 0..0"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_18_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT set_bit('\\x00'::BYTEA,0,2) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-18", steps);
    // Assert
    let frames = &batches[1];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22023"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("new bit must be 0 or 1"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_19_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT set_bit('\\x00'::BYTEA,0,-1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-19", steps);
    // Assert
    let frames = &batches[1];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22023"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("new bit must be 0 or 1"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_20_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT set_bit('\\x00'::BYTEA,9,2) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-20", steps);
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
        Some("index 9 out of valid range, 0..7"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_21_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT encode('x'::TEXT,'hex') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-21", steps);
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
        Some("function encode(text, unknown) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_22_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT decode('00','hex',0) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-22", steps);
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
        Some("function decode(unknown, unknown, integer) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_23_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT get_byte('\\x00'::BYTEA,1::BIGINT) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-23", steps);
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
        Some("function get_byte(bytea, bigint) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_24_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT get_byte('\\x00'::BYTEA,'bad') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-24", steps);
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
fn should_preserve_error_25_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT sha256('x'::TEXT) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-25", steps);
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
        Some("function sha256(text) does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_26_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT get_byte() AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-26", steps);
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
        Some("function get_byte() does not exist"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_27_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('é','hex') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-27", steps);
    // Assert
    let frames = &batches[1];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22023"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("invalid hexadecimal digit: \"é\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_28_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('0é','hex') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-28", steps);
    // Assert
    let frames = &batches[1];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22023"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("invalid hexadecimal digit: \"é\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_29_given_fixed_bytea_input_when_parse_validates_it() {
    // Arrange
    let sql = "SELECT '\\xé'::BYTEA AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-29", steps);
    // Assert
    let frames = &batches[0];
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("22023"),
        "{sql}: {batches:?}"
    );
    let body = &frames.iter().find(|frame| frame.0 == b'E').unwrap().1;
    assert_eq!(
        wire::parse_error_fields(body)
            .iter()
            .find(|field| field.0 == 'M')
            .map(|field| field.1.as_str()),
        Some("invalid hexadecimal digit: \"é\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}
