//! Native fixed BYTEA error timing and diagnostics.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_error_0_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT encode('\\x'::BYTEA,'bad') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-0", steps);
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
        Some("unrecognized encoding: \"bad\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_1_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('','bad') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-1", steps);
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
        Some("unrecognized encoding: \"bad\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_2_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('abc','hex') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-2", steps);
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
        Some("invalid hexadecimal data: odd number of digits"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_3_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('0g','hex') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-3", steps);
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
        Some("invalid hexadecimal digit: \"g\""),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_4_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('MTIzAAE==','base64') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-4", steps);
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
        Some("invalid base64 end sequence"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_5_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('YQ','base64') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-5", steps);
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
        Some("invalid base64 end sequence"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_6_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('YQ===','base64') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-6", steps);
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
        Some("invalid base64 end sequence"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_7_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('!AAA','base64') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-7", steps);
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
        Some("invalid symbol \"!\" found while decoding base64 sequence"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_8_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('AA==A','base64') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-8", steps);
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
        Some("invalid base64 end sequence"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_9_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('\\1','escape') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-9", steps);
    // Assert
    let frames = &batches[1];
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
fn should_preserve_error_10_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT decode('\\x00','escape') AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-10", steps);
    // Assert
    let frames = &batches[1];
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
fn should_preserve_error_11_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_byte('\\x00'::BYTEA,-1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-11", steps);
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
        Some("index -1 out of valid range, 0..0"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_12_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_byte('\\x00'::BYTEA,1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-12", steps);
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
        Some("index 1 out of valid range, 0..0"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}

#[test]
fn should_preserve_error_13_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_byte('\\x'::BYTEA,0) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-13", steps);
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
fn should_preserve_error_14_given_fixed_bytea_input_when_bind_validates_it() {
    // Arrange
    let sql = "SELECT get_bit('\\x00'::BYTEA,-1) AS v";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("", sql)),
        Step::BindFlush(wire::bind_frame("", "", &[])),
        Step::ThroughReady(vec![wire::sync_frame()]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 7")]),
    ];
    // Act
    let batches = run_script("bytea-functions-error-14", steps);
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
        Some("index -1 out of valid range, 0..7"),
        "{frames:?}"
    );
    assert!(!frames.iter().any(|frame| matches!(frame.0, b'D' | b'C')));
    assert_eq!(
        wire::data_rows(batches.last().unwrap()),
        vec![vec![Some("7".into())]]
    );
}
