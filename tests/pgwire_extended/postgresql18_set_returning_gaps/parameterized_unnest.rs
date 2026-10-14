//! Parameterized UNNEST input, ordinality and empty lateral sources for #808.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

const PARAMETERIZED_UNNEST_SQL: &str = "SELECT expanded.value, expanded.ordinality \
    FROM unnest($1::INT4[]) WITH ORDINALITY AS expanded(value, ordinality) \
    ORDER BY expanded.ordinality";

const NONDEFAULT_BOUND_ARRAY_BINARY: &[u8] = b"\0\0\0\x01\0\0\0\x01\0\0\0\x17\0\0\0\x03\0\0\0\0\0\0\0\x04\0\0\0\x07\xff\xff\xff\xff\0\0\0\x04\0\0\0\x09";

fn assert_ordinality_descriptors(frames: &fixture::Frames, portal_format: i16) {
    let descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();

    assert_eq!(descriptions.len(), 2);
    for (index, columns) in descriptions.iter().enumerate() {
        assert_eq!(
            columns
                .iter()
                .map(|column| (column.type_oid, column.type_size, column.type_mod))
                .collect::<Vec<_>>(),
            [(23, 4, -1), (20, 8, -1)]
        );
        let expected_format = if index == 0 { 0 } else { portal_format };
        assert_eq!(
            columns
                .iter()
                .map(|column| column.format_code)
                .collect::<Vec<_>>(),
            [expected_format, expected_format]
        );
    }
}

fn binary_result_row(value: Option<i32>, ordinality: i64) -> Vec<u8> {
    let mut row = Vec::new();
    row.extend_from_slice(&2_i16.to_be_bytes());
    if let Some(value) = value {
        row.extend_from_slice(&4_i32.to_be_bytes());
        row.extend_from_slice(&value.to_be_bytes());
    } else {
        row.extend_from_slice(&(-1_i32).to_be_bytes());
    }
    row.extend_from_slice(&8_i32.to_be_bytes());
    row.extend_from_slice(&ordinality.to_be_bytes());
    row
}

#[test]
fn should_expand_parameterized_array_with_ordinality_from_text_bind() {
    // Arrange
    let cycle = fixture::execute_cycle(
        PARAMETERIZED_UNNEST_SQL,
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-808-parameterized-unnest-text-bind", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_ordinality_descriptors(frames, 0);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("7".to_owned()), Some("1".to_owned())],
            vec![None, Some("2".to_owned())],
            vec![Some("9".to_owned()), Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_expand_binary_bound_array_with_null_element_to_binary_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        PARAMETERIZED_UNNEST_SQL,
        Some((1_007, 1, Some(NONDEFAULT_BOUND_ARRAY_BINARY))),
        1,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-808-parameterized-unnest-binary-bind",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_ordinality_descriptors(frames, 1);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![
            binary_result_row(Some(7), 1),
            binary_result_row(None, 2),
            binary_result_row(Some(9), 3),
        ]
    );
}

#[test]
fn should_preserve_left_rows_for_nonproducing_lateral_unnest_inputs() {
    // Arrange
    let sql = "SELECT source.id, expanded.value, expanded.ordinality \
        FROM (VALUES (1, NULL::INT4[]), (2, ARRAY[]::INT4[])) \
             AS source(id, items) \
        LEFT JOIN LATERAL unnest(source.items) WITH ORDINALITY \
             AS expanded(value, ordinality) ON TRUE \
        ORDER BY source.id, expanded.ordinality";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-808-lateral-unnest-null-empty", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("lateral UNNEST RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), None, None],
            vec![Some("2".to_owned()), None, None],
        ]
    );
}
