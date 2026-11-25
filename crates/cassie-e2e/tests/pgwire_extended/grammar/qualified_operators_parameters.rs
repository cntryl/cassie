//! Native BYTEA concatenation signatures and text/binary parameter crosses.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_signature_0_given_bytea_concatenation_when_parameter_formats_cross() {
    // Arrange
    let parameters: [[&[u8]; 2]; 2] = [
        [&[92, 120, 48, 48, 97, 98], &[92, 120, 102, 102]],
        [&[0, 171], &[255]],
    ];
    let mut steps = vec![Step::ThroughReady(vec![
        wire::parse_frame_with_types("bytes", "SELECT $1 || $2 AS v", &[17, 17]),
        wire::describe_statement_frame("bytes"),
        wire::sync_frame(),
    ])];
    for input_format in [0_i16, 1] {
        for result_format in [0_i16, 1] {
            let values = parameters[usize::try_from(input_format).unwrap()].map(Some);
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "",
                    "bytes",
                    &[input_format],
                    &values,
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }
    // Act
    let batches = run_script("bytea-concat-parameters-0", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 17]);
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 171, 255],
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 171, 255],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (17, -1, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_1_given_bytea_concatenation_when_parameter_formats_cross() {
    // Arrange
    let parameters: [[&[u8]; 2]; 2] = [
        [&[92, 120, 48, 48, 97, 98], &[92, 120, 102, 102]],
        [&[0, 171], &[255]],
    ];
    let mut steps = vec![Step::ThroughReady(vec![
        wire::parse_frame_with_types("bytes", "SELECT $1 || $2 AS v", &[17, 0]),
        wire::describe_statement_frame("bytes"),
        wire::sync_frame(),
    ])];
    for input_format in [0_i16, 1] {
        for result_format in [0_i16, 1] {
            let values = parameters[usize::try_from(input_format).unwrap()].map(Some);
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "",
                    "bytes",
                    &[input_format],
                    &values,
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }
    // Act
    let batches = run_script("bytea-concat-parameters-1", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 17]);
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 171, 255],
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 171, 255],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (17, -1, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_2_given_bytea_concatenation_when_parameter_formats_cross() {
    // Arrange
    let parameters: [[&[u8]; 2]; 2] = [
        [&[92, 120, 48, 48, 97, 98], &[92, 120, 102, 102]],
        [&[0, 171], &[255]],
    ];
    let mut steps = vec![Step::ThroughReady(vec![
        wire::parse_frame_with_types("bytes", "SELECT $1 || $2 AS v", &[0, 17]),
        wire::describe_statement_frame("bytes"),
        wire::sync_frame(),
    ])];
    for input_format in [0_i16, 1] {
        for result_format in [0_i16, 1] {
            let values = parameters[usize::try_from(input_format).unwrap()].map(Some);
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "",
                    "bytes",
                    &[input_format],
                    &values,
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }
    // Act
    let batches = run_script("bytea-concat-parameters-2", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 17]);
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 171, 255],
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 97, 98, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 171, 255],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (17, -1, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_3_given_bytea_concatenation_when_parameter_formats_cross() {
    // Arrange
    let parameters: [[&[u8]; 2]; 2] = [[&[92, 120, 48, 48, 97, 98], &[116]], [&[0, 171], &[116]]];
    let mut steps = vec![Step::ThroughReady(vec![
        wire::parse_frame_with_types("bytes", "SELECT $1 || $2 AS v", &[17, 25]),
        wire::describe_statement_frame("bytes"),
        wire::sync_frame(),
    ])];
    for input_format in [0_i16, 1] {
        for result_format in [0_i16, 1] {
            let values = parameters[usize::try_from(input_format).unwrap()].map(Some);
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "",
                    "bytes",
                    &[input_format],
                    &values,
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }
    // Act
    let batches = run_script("bytea-concat-parameters-3", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 25]);
    fixture::assert_single_column_descriptors(&batches[0], (25, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116],
        &[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116],
        &[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116],
        &[0, 1, 0, 0, 0, 7, 92, 120, 48, 48, 97, 98, 116],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (25, -1, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_4_given_bytea_concatenation_when_parameter_formats_cross() {
    // Arrange
    let parameters: [[&[u8]; 2]; 2] = [[&[116], &[92, 120, 102, 102]], [&[116], &[255]]];
    let mut steps = vec![Step::ThroughReady(vec![
        wire::parse_frame_with_types("bytes", "SELECT $1 || $2 AS v", &[25, 17]),
        wire::describe_statement_frame("bytes"),
        wire::sync_frame(),
    ])];
    for input_format in [0_i16, 1] {
        for result_format in [0_i16, 1] {
            let values = parameters[usize::try_from(input_format).unwrap()].map(Some);
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "",
                    "bytes",
                    &[input_format],
                    &values,
                    &[result_format],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }
    // Act
    let batches = run_script("bytea-concat-parameters-4", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [25, 17]);
    fixture::assert_single_column_descriptors(&batches[0], (25, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 5, 116, 92, 120, 102, 102],
        &[0, 1, 0, 0, 0, 5, 116, 92, 120, 102, 102],
        &[0, 1, 0, 0, 0, 5, 116, 92, 120, 102, 102],
        &[0, 1, 0, 0, 0, 5, 116, 92, 120, 102, 102],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (25, -1, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}
