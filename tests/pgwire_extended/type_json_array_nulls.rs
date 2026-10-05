//! ARRAY element classes come from independently written raw wire payloads.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

const SQL_NULL_ARRAY: &[u8] = &[
    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 114, 0, 0, 0, 1, 0, 0, 0, 1, 255, 255, 255, 255,
];

// A real SQL NULL makes has_null1 unambiguously valid. The second element is
// a non-NULL four-byte JSON document null, not a second SQL NULL length−1.
const SQL_NULL_WITH_DOCUMENT_NULL: &[u8] = &[
    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 114, 0, 0, 0, 2, 0, 0, 0, 1, 255, 255, 255, 255, 0, 0, 0, 4,
    b'n', b'u', b'l', b'l',
];

const NESTED_NULL_DOCUMENT_ARRAY: &[u8] = &[
    0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 114, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 6, b'[', b'n', b'u',
    b'l', b'l', b']',
];

const STRING_NULL_DOCUMENT_ARRAY: &[u8] = &[
    0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 114, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 6, b'"', b'n', b'u',
    b'l', b'l', b'"',
];

#[test]
fn should_preserve_json_array_wire_identity_across_suspended_pages() {
    // Arrange
    let sql = "WITH docs AS (SELECT $1 AS doc) SELECT doc FROM docs UNION ALL SELECT doc FROM docs";
    let cycle = vec![
        wire::parse_frame_with_types("", sql, &[34_114]),
        wire::describe_statement_frame(""),
        wire::bind_frame_with_formats("", "", &[1], &[Some(NESTED_NULL_DOCUMENT_ARRAY)], &[1]),
        wire::describe_portal_frame(""),
        wire::execute_limited_frame("", 1),
        wire::execute_limited_frame("", 1),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("type-json-array-suspended-descriptors", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (34_114, -1, -1), 1, true);
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b's').count(), 1);
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 1);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(NESTED_NULL_DOCUMENT_ARRAY); 2]
    );
}

#[test]
fn should_reject_document_null_elements_in_binary_json_arrays() {
    // Arrange
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle(
                "SELECT $1 AS docs",
                Some((34_114, 1, Some(SQL_NULL_WITH_DOCUMENT_NULL))),
                format,
                false,
            )
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-json-array-document-null-binary", &[], cycles);

    // Assert
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_reject_document_null_elements_in_text_json_arrays() {
    // Arrange
    // The unquoted NULL is SQL NULL. Quoted null is a JSON document payload.
    let cases = [&br#"{"null"}"#[..], &br#"{NULL,"null"}"#[..]];
    let cycles = cases
        .into_iter()
        .flat_map(|body| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    "SELECT $1 AS docs",
                    Some((34_114, 0, Some(body))),
                    format,
                    false,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-array-document-null-text", &[], cycles);

    // Assert
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_preserve_sql_null_elements_in_json_arrays() {
    // Arrange
    let cases = [(0, &b"{NULL}"[..]), (1, SQL_NULL_ARRAY)];
    let cycles = cases
        .into_iter()
        .flat_map(|(input_format, body)| {
            [0, 1].map(|result_format| {
                fixture::execute_cycle(
                    "SELECT $1 AS docs",
                    Some((34_114, input_format, Some(body))),
                    result_format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-array-sql-null", &[], cycles);

    // Assert
    for pair in batches.as_chunks::<2>().0 {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (34_114, -1, -1), format, true);
            let expected = if format == 0 {
                fixture::one_field_row(b"[null]")
            } else {
                fixture::one_field_row(SQL_NULL_ARRAY)
            };
            assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
        }
    }
}

#[test]
fn should_preserve_nonnull_json_array_documents_containing_null() {
    // Arrange
    let cases = [
        (NESTED_NULL_DOCUMENT_ARRAY, &b"[[null]]"[..]),
        (STRING_NULL_DOCUMENT_ARRAY, &br#"["null"]"#[..]),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(body, _)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    "SELECT $1 AS docs",
                    Some((34_114, 1, Some(body))),
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-array-nested-null-documents", &[], cycles);

    // Assert
    for ((body, text), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (34_114, -1, -1), format, true);
            let expected = fixture::one_field_row(if format == 0 { text } else { body });
            assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
        }
    }
}

#[test]
fn should_distinguish_json_document_null_from_sql_null_on_wire() {
    // Arrange
    let inputs = [(0, Some(&b"null"[..])), (1, Some(&b"null"[..])), (1, None)];
    let cycles = inputs
        .into_iter()
        .flat_map(|(input_format, body)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    "SELECT $1 AS doc",
                    Some((114, input_format, body)),
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-top-level-json-null", &[], cycles);

    // Assert
    for ((_, input), pair) in inputs.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (114, -1, -1), format, true);
            let expected =
                input.map_or_else(|| vec![0, 1, 255, 255, 255, 255], fixture::one_field_row);
            assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
        }
    }
}

#[test]
fn should_preserve_json_null_predicate_classification() {
    // Arrange
    let inputs = [
        (114, 0, Some(&b"null"[..]), b"f".as_slice()),
        (114, 1, Some(&b"null"[..]), b"f".as_slice()),
        (114, 1, None, b"t".as_slice()),
        (25, 0, Some(&b"NULL"[..]), b"f".as_slice()),
    ];
    let cycles = inputs
        .map(|(oid, input_format, body, _)| {
            fixture::execute_cycle(
                "SELECT $1 IS NULL AS is_null",
                Some((oid, input_format, body)),
                0,
                true,
            )
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-json-null-predicate", &[], cycles);

    // Assert
    for ((_, _, _, expected), frames) in inputs.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (16, 1, -1), 0, true);
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(expected)]
        );
    }
}

#[test]
fn should_preserve_json_array_text_null_slot_origin() {
    // Arrange
    let cases = [
        (&b"[null]"[..], SQL_NULL_ARRAY),
        (&b"[[null]]"[..], NESTED_NULL_DOCUMENT_ARRAY),
        (&br#"["null"]"#[..], STRING_NULL_DOCUMENT_ARRAY),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(body, _)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    "SELECT $1 AS docs",
                    Some((34_114, 0, Some(body))),
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-array-bracket-null-origin", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for ((text, binary), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (34_114, -1, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(if format == 0 {
                    text
                } else {
                    binary
                })]
            );
        }
    }
}

#[test]
fn should_reject_nonnull_json_document_null_without_sql_null_frames() {
    // Arrange
    // One non-NULL OID114 payload; has_null=0 is valid because there is no −1.
    let body = [
        0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 114, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 4, b'n', b'u', b'l',
        b'l',
    ];
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle(
                "SELECT $1 AS docs",
                Some((34_114, 1, Some(&body))),
                format,
                false,
            )
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-json-document-null-no-sql-null", &[], cycles);

    // Assert
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_preserve_malformed_json_array_error_classification() {
    // Arrange
    let mut trailing = SQL_NULL_WITH_DOCUMENT_NULL.to_vec();
    trailing.push(0);
    let mut truncated = SQL_NULL_WITH_DOCUMENT_NULL.to_vec();
    truncated[12..16].copy_from_slice(&3_i32.to_be_bytes());
    truncated.extend_from_slice(&4_i32.to_be_bytes());
    truncated.extend_from_slice(b"tru");
    let mut invalid_json = SQL_NULL_WITH_DOCUMENT_NULL.to_vec();
    invalid_json[12..16].copy_from_slice(&3_i32.to_be_bytes());
    invalid_json.extend_from_slice(&6_i32.to_be_bytes());
    invalid_json.extend_from_slice(b"broken");
    let mut false_null_flag = SQL_NULL_WITH_DOCUMENT_NULL.to_vec();
    false_null_flag[7] = 0;
    let cases = [
        (0, &br#"{"null"}junk"#[..]),
        (0, &br#"{"null","broken"}"#[..]),
        (1, trailing.as_slice()),
        (1, truncated.as_slice()),
        (1, invalid_json.as_slice()),
        (1, false_null_flag.as_slice()),
    ];
    let cycles = cases
        .into_iter()
        .map(|(format, body)| {
            fixture::execute_cycle(
                "SELECT $1 AS docs",
                Some((34_114, format, Some(body))),
                0,
                false,
            )
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-array-malformed-priority", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("08P01"));
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        assert!(frames
            .iter()
            .all(|(tag, _)| !matches!(*tag, b'T' | b'D' | b'C')));
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
}
