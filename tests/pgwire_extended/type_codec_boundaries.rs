//! Finite malformed-input classes missing from the mapped codec owners.

use super::support_pgwire as wire;
use super::support_type_codec_matrix as matrix;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_reject_invalid_fixed_scalar_binary_widths() {
    // Arrange
    let mut cycles = Vec::new();
    for case in matrix::SCALAR_CASES.iter().filter(|case| case.typlen > 0) {
        let mut short = case.input_binary.to_vec();
        short.pop().expect("fixed scalar literal is nonempty");
        let mut long = case.input_binary.to_vec();
        long.push(0);
        let sql = format!("SELECT CAST($1 AS {}) AS v", case.sql_type);
        for body in [short, long] {
            cycles.push(fixture::execute_cycle(
                &sql,
                Some((case.oid, 1, Some(&body))),
                0,
                false,
            ));
        }
    }

    // Act
    let batches = fixture::run_cycles("type-fixed-scalar-width-errors", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 18);
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("08P01"));
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'T').count(), 0);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}

#[test]
fn should_reject_out_of_range_temporal_binary_parameters() {
    // Arrange
    // Invalid finite endpoints, not a newly selected infinity/BC codec.
    let cases: [(i32, &[u8]); 6] = [
        (1082, &[128, 0, 0, 0]),
        (1082, &[127, 255, 255, 255]),
        (1083, &[255, 255, 255, 255, 255, 255, 255, 255]),
        (1083, &[0, 0, 0, 20, 29, 215, 96, 0]),
        (1114, &[128, 0, 0, 0, 0, 0, 0, 0]),
        (1114, &[127, 255, 255, 255, 255, 255, 255, 255]),
    ];
    let cycles = cases
        .map(|(oid, body)| {
            fixture::execute_cycle("SELECT $1 AS v", Some((oid, 1, Some(body))), 0, false)
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-temporal-binary-range-errors", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("08P01"));
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'T').count(), 0);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}

#[test]
fn should_reject_malformed_binary_vector_headers() {
    // Arrange
    let cases: [&[u8]; 7] = [
        &[0, 3, 0, 0, 63, 192, 0, 0, 192, 0, 0, 0],
        &[0, 2, 0, 1, 63, 192, 0, 0, 192, 0, 0, 0],
        &[0, 2, 0, 0, 127, 192, 0, 0, 192, 0, 0, 0],
        &[0, 2, 0, 0, 127, 128, 0, 0, 192, 0, 0, 0],
        &[0, 2, 0, 0, 63, 192, 0, 0, 192, 0, 0],
        &[0, 2, 0, 0, 63, 192, 0, 0, 192, 0, 0, 0, 0],
        &[0, 0, 0, 0],
    ];
    let cycles = cases
        .map(|body| {
            fixture::execute_cycle("SELECT $1 AS v", Some((100_002, 1, Some(body))), 0, false)
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-vector-binary-header-errors", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 7);
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("08P01"));
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'T').count(), 0);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}

#[test]
fn should_reject_malformed_scalar_array_headers() {
    // Arrange
    // INT[]34023 is a supported representative of the single shared decoder.
    let valid = [
        0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 23, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0, 7, 255,
        255, 255, 255,
    ];
    let changes = [
        (0, [0, 0, 0, 2]),
        (4, [0, 0, 0, 2]),
        (4, [0, 0, 0, 0]),
        (8, [0, 0, 0, 20]),
        (12, [255, 255, 255, 255]),
        (12, [127, 255, 255, 255]),
        (16, [0, 0, 0, 0]),
        (20, [0, 0, 0, 5]),
    ];
    let mut bodies = changes
        .map(|(offset, bytes)| {
            let mut body = valid.to_vec();
            body[offset..offset + 4].copy_from_slice(&bytes);
            body
        })
        .to_vec();
    let mut trailing = valid.to_vec();
    trailing.push(0);
    bodies.push(trailing);
    let cycles = bodies
        .iter()
        .map(|body| {
            fixture::execute_cycle("SELECT $1 AS v", Some((34_023, 1, Some(body))), 0, false)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-binary-header-errors", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 9);
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("08P01"));
        assert_eq!(fixture::data_row_payloads(&frames), Vec::<Vec<u8>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'T').count(), 0);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}

#[test]
fn should_reject_unregistered_binary_array_identities() {
    // Arrange
    let body = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 23];
    let cycles = vec![fixture::execute_cycle(
        "SELECT CAST($1 AS INT[]) AS v",
        Some((1007, 1, Some(&body))),
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("type-unregistered-array-oid", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 1);
    fixture::assert_unsupported_before_metadata(&batches[0]);
}

#[test]
fn should_preserve_unsigned_json_documents_in_scalar_arrays() {
    // Arrange
    let binary = [
        0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 114, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 20, b'1', b'8',
        b'4', b'4', b'6', b'7', b'4', b'4', b'0', b'7', b'3', b'7', b'0', b'9', b'5', b'5', b'1',
        b'6', b'1', b'5', 255, 255, 255, 255,
    ];
    let cases = [(0, &b"{18446744073709551615,NULL}"[..]), (1, &binary[..])];
    let cycles = cases
        .into_iter()
        .flat_map(|(input_format, body)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((34_114, input_format, Some(body))),
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-array-json-unsigned-number", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for pair in batches.as_chunks::<2>().0 {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (34_114, -1, -1), format, true);
            let expected = fixture::one_field_row(if format == 0 {
                b"[18446744073709551615,null]"
            } else {
                &binary
            });
            assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
        }
    }
}

#[test]
fn should_round_trip_integer_binary_endpoints() {
    // Arrange
    type IntegerEndpoint = (&'static str, i32, i16, &'static [u8], &'static [u8]);
    let cases: [IntegerEndpoint; 4] = [
        ("SMALLINT", 21, 2, b"32767", &[127, 255]),
        ("INT", 23, 4, b"2147483647", &[127, 255, 255, 255]),
        (
            "BIGINT",
            20,
            8,
            b"-9223372036854775808",
            &[128, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            "BIGINT",
            20,
            8,
            b"9223372036854775807",
            &[127, 255, 255, 255, 255, 255, 255, 255],
        ),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (sql_type, oid, width, text, binary) in cases {
        let sql = format!("SELECT CAST($1 AS {sql_type}) AS v");
        for (input_format, input) in [(0, text), (1, binary)] {
            for (output_format, output) in [(0, text), (1, binary)] {
                for described in [false, true] {
                    cycles.push(fixture::execute_cycle(
                        &sql,
                        Some((oid, input_format, Some(input))),
                        output_format,
                        described,
                    ));
                    expectations.push((
                        oid,
                        width,
                        output_format,
                        described,
                        fixture::one_field_row(output),
                    ));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-integer-binary-endpoints", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 32);
    for (frames, (oid, width, format, described, expected)) in batches.iter().zip(expectations) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, width, -1), format, described);
        assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
    }
}

#[test]
fn should_round_trip_finite_time_binary_endpoints() {
    // Arrange
    // Midnight and the last finite microsecond before the next day.
    let cases: [(&[u8], &[u8]); 2] = [
        (b"00:00:00", &[0, 0, 0, 0, 0, 0, 0, 0]),
        (b"23:59:59.999999", &[0, 0, 0, 20, 29, 215, 95, 255]),
    ];
    let mut cycles = Vec::new();
    let mut expectations = Vec::new();
    for (text, binary) in cases {
        for (input_format, input) in [(0, text), (1, binary)] {
            for (output_format, output) in [(0, text), (1, binary)] {
                for described in [false, true] {
                    cycles.push(fixture::execute_cycle(
                        "SELECT CAST($1 AS TIME) AS v",
                        Some((1083, input_format, Some(input))),
                        output_format,
                        described,
                    ));
                    expectations.push((output_format, described, fixture::one_field_row(output)));
                }
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-finite-time-binary-endpoints", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 16);
    for (frames, (format, described, expected)) in batches.iter().zip(expectations) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (1083, 8, -1), format, described);
        assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
    }
}

#[test]
fn should_reject_noncanonical_boolean_binary_byte() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT CAST($1 AS BOOLEAN) AS v",
        Some((16, 1, Some(&[2]))),
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("type-boolean-binary-byte-error", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 1);
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames).as_deref(), Some("08P01"));
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    assert_eq!(
        frames
            .iter()
            .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
            .count(),
        0
    );
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
}

#[test]
fn should_reject_invalid_scalar_array_element_lengths() {
    // Arrange
    let negative_element = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 23, 0, 0, 0, 1, 0, 0, 0, 1, 255, 255, 255, 254,
    ];
    let truncated_header = [0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
    let cycles = [&negative_element[..], &truncated_header[..]]
        .map(|body| {
            fixture::execute_cycle("SELECT $1 AS v", Some((34_023, 1, Some(body))), 0, false)
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-array-binary-length-errors", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 2);
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("08P01"));
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert_eq!(
            frames
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}

#[test]
fn should_reject_unregistered_vector_binary_results_before_writes() {
    // Arrange
    // The declared storage/text width exceeds the selected binary OID family.
    // NULL avoids allocating any 32,768-component vector payload.
    let setup = [(
        "CREATE TABLE wide_vector_null (v VECTOR(32768))",
        Vec::new(),
    )];
    let cycles = vec![
        fixture::execute_cycle(
            "INSERT INTO wide_vector_null (v) VALUES (NULL) RETURNING v",
            None,
            1,
            false,
        ),
        fixture::execute_cycle("SELECT COUNT(*) AS v FROM wide_vector_null", None, 1, false),
        fixture::execute_cycle("SELECT v FROM wide_vector_null", None, 1, false),
        fixture::execute_cycle(
            "INSERT INTO wide_vector_null (v) VALUES (NULL) RETURNING v",
            None,
            0,
            true,
        ),
        fixture::execute_cycle("SELECT v FROM wide_vector_null", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("type-wide-vector-binary-prewrite", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 5);
    fixture::assert_unsupported_before_metadata(&batches[0]);
    fixture::assert_success(&batches[1]);
    fixture::assert_single_column_descriptors(&batches[1], (20, 8, -1), 1, false);
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&[0, 0, 0, 0, 0, 0, 0, 0])],
        "binary RETURNING rejection must leave the table empty",
    );
    fixture::assert_unsupported_before_metadata(&batches[2]);
    for frames in [&batches[3], &batches[4]] {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (132_768, -1, -1), 0, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![vec![0, 1, 255, 255, 255, 255]],
            "text RETURNING and stored read retain one SQL NULL field",
        );
    }
}
