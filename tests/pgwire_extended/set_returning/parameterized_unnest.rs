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

include!("parameterized_unnest/parameters.rs");

include!("../types/arrays/parameterized_unnest/binary.rs");

include!("../types/arrays/parameterized_unnest/semantics.rs");
