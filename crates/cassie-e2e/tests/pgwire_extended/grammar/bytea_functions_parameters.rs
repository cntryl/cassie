//! Fixed BYTEA signatures with both parameter and result formats.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_signature_0_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT encode($1,$2) AS v", &[]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[
                    Some(&[92, 120, 48, 48, 102, 102]),
                    Some(&[98, 97, 115, 101, 54, 52]),
                ],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[
                    Some(&[92, 120, 48, 48, 102, 102]),
                    Some(&[98, 97, 115, 101, 54, 52]),
                ],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[0, 255]), Some(&[98, 97, 115, 101, 54, 52])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[0, 255]), Some(&[98, 97, 115, 101, 54, 52])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-0", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 25]);
    fixture::assert_single_column_descriptors(&batches[0], (25, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 4, 65, 80, 56, 61],
        &[0, 1, 0, 0, 0, 4, 65, 80, 56, 61],
        &[0, 1, 0, 0, 0, 4, 65, 80, 56, 61],
        &[0, 1, 0, 0, 0, 4, 65, 80, 56, 61],
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
fn should_preserve_signature_1_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT decode($1,$2) AS v", &[]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[65, 65, 47, 47]), Some(&[98, 97, 115, 101, 54, 52])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[65, 65, 47, 47]), Some(&[98, 97, 115, 101, 54, 52])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[65, 65, 47, 47]), Some(&[98, 97, 115, 101, 54, 52])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[65, 65, 47, 47]), Some(&[98, 97, 115, 101, 54, 52])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-1", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [25, 25]);
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 48, 102, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 15, 255],
        &[0, 1, 0, 0, 0, 8, 92, 120, 48, 48, 48, 102, 102, 102],
        &[0, 1, 0, 0, 0, 3, 0, 15, 255],
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
fn should_preserve_signature_2_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT get_byte($1,$2) AS v", &[17, 0]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 56, 48, 102, 102]), Some(&[49])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 56, 48, 102, 102]), Some(&[49])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[128, 255]), Some(&[0, 0, 0, 1])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[128, 255]), Some(&[0, 0, 0, 1])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-2", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 23]);
    fixture::assert_single_column_descriptors(&batches[0], (23, 4, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 3, 50, 53, 53],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 255],
        &[0, 1, 0, 0, 0, 3, 50, 53, 53],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 255],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (23, 4, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_3_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT get_bit($1,$2) AS v", &[17, 0]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 48, 48, 48, 49]), Some(&[56])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 48, 48, 48, 49]), Some(&[56])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[0, 1]), Some(&[0, 0, 0, 0, 0, 0, 0, 8])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[0, 1]), Some(&[0, 0, 0, 0, 0, 0, 0, 8])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-3", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 20]);
    fixture::assert_single_column_descriptors(&batches[0], (23, 4, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 1, 49],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
        &[0, 1, 0, 0, 0, 1, 49],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (23, 4, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_4_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT get_bit($1,$2) AS v", &[17, 23]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 56, 48]), Some(&[55])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 56, 48]), Some(&[55])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[128]), Some(&[0, 0, 0, 7])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[Some(&[128]), Some(&[0, 0, 0, 7])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-4", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17, 23]);
    fixture::assert_single_column_descriptors(&batches[0], (23, 4, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 1, 49],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
        &[0, 1, 0, 0, 0, 1, 49],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (23, 4, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_5_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT set_byte($1,$2,$3) AS v", &[]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[
                    Some(&[92, 120, 48, 48, 102, 102]),
                    Some(&[48]),
                    Some(&[45, 49]),
                ],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[
                    Some(&[92, 120, 48, 48, 102, 102]),
                    Some(&[48]),
                    Some(&[45, 49]),
                ],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[
                    Some(&[0, 255]),
                    Some(&[0, 0, 0, 0]),
                    Some(&[255, 255, 255, 255]),
                ],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[
                    Some(&[0, 255]),
                    Some(&[0, 0, 0, 0]),
                    Some(&[255, 255, 255, 255]),
                ],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-5", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        [17, 23, 23]
    );
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 6, 92, 120, 102, 102, 102, 102],
        &[0, 1, 0, 0, 0, 2, 255, 255],
        &[0, 1, 0, 0, 0, 6, 92, 120, 102, 102, 102, 102],
        &[0, 1, 0, 0, 0, 2, 255, 255],
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
fn should_preserve_signature_6_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT set_bit($1,$2,$3) AS v", &[17, 0, 0]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 48, 48, 102, 102]), Some(&[56]), Some(&[48])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 48, 48, 102, 102]), Some(&[56]), Some(&[48])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[
                    Some(&[0, 255]),
                    Some(&[0, 0, 0, 0, 0, 0, 0, 8]),
                    Some(&[0, 0, 0, 0]),
                ],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[1],
                &[
                    Some(&[0, 255]),
                    Some(&[0, 0, 0, 0, 0, 0, 0, 8]),
                    Some(&[0, 0, 0, 0]),
                ],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-6", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        [17, 20, 23]
    );
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 102, 101],
        &[0, 1, 0, 0, 0, 2, 0, 254],
        &[0, 1, 0, 0, 0, 6, 92, 120, 48, 48, 102, 101],
        &[0, 1, 0, 0, 0, 2, 0, 254],
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
fn should_preserve_signature_7_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT sha256($1) AS v", &[]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 54, 49, 54, 50, 54, 51])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 54, 49, 54, 50, 54, 51])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[97, 98, 99])], &[0]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[97, 98, 99])], &[1]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-7", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17]);
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[
            0, 1, 0, 0, 0, 66, 92, 120, 98, 97, 55, 56, 49, 54, 98, 102, 56, 102, 48, 49, 99, 102,
            101, 97, 52, 49, 52, 49, 52, 48, 100, 101, 53, 100, 97, 101, 50, 50, 50, 51, 98, 48,
            48, 51, 54, 49, 97, 51, 57, 54, 49, 55, 55, 97, 57, 99, 98, 52, 49, 48, 102, 102, 54,
            49, 102, 50, 48, 48, 49, 53, 97, 100,
        ],
        &[
            0, 1, 0, 0, 0, 32, 186, 120, 22, 191, 143, 1, 207, 234, 65, 65, 64, 222, 93, 174, 34,
            35, 176, 3, 97, 163, 150, 23, 122, 156, 180, 16, 255, 97, 242, 0, 21, 173,
        ],
        &[
            0, 1, 0, 0, 0, 66, 92, 120, 98, 97, 55, 56, 49, 54, 98, 102, 56, 102, 48, 49, 99, 102,
            101, 97, 52, 49, 52, 49, 52, 48, 100, 101, 53, 100, 97, 101, 50, 50, 50, 51, 98, 48,
            48, 51, 54, 49, 97, 51, 57, 54, 49, 55, 55, 97, 57, 99, 98, 52, 49, 48, 102, 102, 54,
            49, 102, 50, 48, 48, 49, 53, 97, 100,
        ],
        &[
            0, 1, 0, 0, 0, 32, 186, 120, 22, 191, 143, 1, 207, 234, 65, 65, 64, 222, 93, 174, 34,
            35, 176, 3, 97, 163, 150, 23, 122, 156, 180, 16, 255, 97, 242, 0, 21, 173,
        ],
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
fn should_preserve_signature_8_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT bit_count($1) AS v", &[17]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 48, 48, 102, 102])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 48, 48, 102, 102])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[0, 255])], &[0]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[0, 255])], &[1]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-8", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17]);
    fixture::assert_single_column_descriptors(&batches[0], (20, 8, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 1, 56],
        &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 8],
        &[0, 1, 0, 0, 0, 1, 56],
        &[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 8],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (20, 8, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_9_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT get_bit($1,0::BIGINT) AS v", &[]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 54, 49, 54, 50, 54, 51])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 54, 49, 54, 50, 54, 51])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[97, 98, 99])], &[0]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[97, 98, 99])], &[1]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-9", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17]);
    fixture::assert_single_column_descriptors(&batches[0], (23, 4, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 1, 49],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
        &[0, 1, 0, 0, 0, 1, 49],
        &[0, 1, 0, 0, 0, 4, 0, 0, 0, 1],
    ];
    for (cycle, (frames, payload)) in batches[1..].iter().zip(expected).enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(
            frames,
            (23, 4, -1),
            i16::try_from(cycle % 2).unwrap(),
            false,
        );
        assert_eq!(fixture::data_row_payloads(frames), vec![payload.to_vec()]);
    }
}

#[test]
fn should_preserve_signature_10_given_fixed_bytea_parameters_when_wire_formats_cross() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![
            wire::parse_frame_with_types("bytes", "SELECT set_bit($1,0::BIGINT,0) AS v", &[]),
            wire::describe_statement_frame("bytes"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 54, 49, 54, 50, 54, 51])],
                &[0],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "",
                "bytes",
                &[0],
                &[Some(&[92, 120, 54, 49, 54, 50, 54, 51])],
                &[1],
            ),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[97, 98, 99])], &[0]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("", "bytes", &[1], &[Some(&[97, 98, 99])], &[1]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]),
    ];
    // Act
    let batches = run_script("bytea-function-parameters-10", steps);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0].iter().find(|frame| frame.0 == b't').unwrap();
    assert_eq!(wire::parse_parameter_description(&parameters.1), [17]);
    fixture::assert_single_column_descriptors(&batches[0], (17, -1, -1), 0, false);
    let expected: [&[u8]; 4] = [
        &[0, 1, 0, 0, 0, 8, 92, 120, 54, 48, 54, 50, 54, 51],
        &[0, 1, 0, 0, 0, 3, 96, 98, 99],
        &[0, 1, 0, 0, 0, 8, 92, 120, 54, 48, 54, 50, 54, 51],
        &[0, 1, 0, 0, 0, 3, 96, 98, 99],
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
