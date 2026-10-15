//! Finite CBC2 dictionary corruption witnesses using the current decoder format.
use cassie::app::CassieError;
use cassie::midge::adapter::{
    column_chunk_codec_for_test, decode_column_chunk_for_test,
    decode_selected_column_chunk_for_test,
};
use serde_json::{json, Value};

type DecodedViews = [(&'static str, Result<Vec<Value>, CassieError>); 3];

fn dictionary_twin() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(68);
    bytes.extend_from_slice(b"CBC2");
    bytes.extend_from_slice(&2_u16.to_le_bytes()); // Current format version.
    bytes.extend_from_slice(&[4, 3]); // UTF-8 logical type, dictionary codec.
    bytes.extend_from_slice(&0_u16.to_le_bytes()); // Flags.
    for value in [2_u32, 0, 1, 33] {
        // Values, NULLs, validity bytes, payload bytes.
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&10_u64.to_le_bytes()); // Two plain length-prefixed scalars.
    bytes.push(0x03); // Both rows valid; unused bits clear.
    bytes.extend_from_slice(&2_u32.to_le_bytes()); // Dictionary entries.
    for &scalar in b"ab" {
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.push(scalar);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes()); // One FOR index block.
    bytes.push(2); // Two indices.
    bytes.extend_from_slice(&0_u64.to_le_bytes()); // Base index.
    bytes.push(2); // Two bits per index, accepted by the current decoder.
    bytes.extend_from_slice(&1_u32.to_le_bytes()); // Packed length.
    bytes.push(0x04); // Low bits first: indices zero, one.
    bytes
}

fn decode_views(bytes: &[u8]) -> DecodedViews {
    [
        ("full", decode_column_chunk_for_test(bytes)),
        (
            "sparse",
            decode_selected_column_chunk_for_test(bytes, &[true, false]),
        ),
        (
            "all-false",
            decode_selected_column_chunk_for_test(bytes, &[false, false]),
        ),
    ]
}

fn assert_valid_twin(bytes: &[u8], codec: Result<&str, CassieError>, views: DecodedViews) {
    assert_eq!(bytes.len(), 68);
    assert_eq!(
        codec.expect("valid current-format dictionary codec"),
        "dictionary"
    );
    let expected = [
        vec![json!("a"), json!("b")],
        vec![json!("a"), Value::Null],
        vec![Value::Null, Value::Null],
    ];
    for ((selection, actual), expected) in views.into_iter().zip(expected) {
        assert_eq!(
            actual.unwrap_or_else(|error| panic!("valid {selection}: {error}")),
            expected,
            "valid {selection}"
        );
    }
}

fn assert_one_byte_fault(
    valid: &[u8],
    corrupt: &[u8],
    offset: usize,
    views: DecodedViews,
    diagnostic: &str,
) {
    assert_eq!(corrupt.len(), valid.len());
    let differences = valid
        .iter()
        .zip(corrupt)
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some(index))
        .collect::<Vec<_>>();
    assert_eq!(differences, vec![offset]);
    for (selection, result) in views {
        let error = result.expect_err("complete chunk validation precedes selection");
        assert!(
            matches!(error, CassieError::Parse(ref message) if message==diagnostic),
            "{selection}: {error:?}; expected Parse({diagnostic:?})"
        );
    }
}

#[test]
fn should_reject_an_unselected_dictionary_index_out_of_range() {
    // Arrange
    let valid = dictionary_twin();
    let mut corrupt = valid.clone();
    corrupt[67] = 0x0c; // First index remains zero; unselected second index becomes three.

    // Act
    let codec = column_chunk_codec_for_test(&valid);
    let valid_views = decode_views(&valid);
    let corrupt_views = decode_views(&corrupt);
    // Assert
    assert_valid_twin(&valid, codec, valid_views);
    assert_one_byte_fault(
        &valid,
        &corrupt,
        67,
        corrupt_views,
        "column-batch dictionary index out of range",
    );
}

#[test]
fn should_reject_invalid_utf8_in_an_unselected_dictionary_entry() {
    // Arrange
    let valid = dictionary_twin();
    let mut corrupt = valid.clone();
    corrupt[48] = 0xff; // One invalid scalar byte; its original length remains one.

    // Act
    let codec = column_chunk_codec_for_test(&valid);
    let valid_views = decode_views(&valid);
    let corrupt_views = decode_views(&corrupt);
    // Assert
    assert_valid_twin(&valid, codec, valid_views);
    assert!(
        corrupt[43] < corrupt[48],
        "raw dictionary order remains strictly sorted"
    );
    assert_one_byte_fault(
        &valid,
        &corrupt,
        48,
        corrupt_views,
        "invalid column-batch UTF-8",
    );
}

#[test]
fn should_reject_unused_validity_bits_before_selected_dictionary_decode() {
    // Arrange
    let valid = dictionary_twin();
    let mut corrupt = valid.clone();
    corrupt[34] = 0x83; // Both live bits remain set; unused bit seven is now invalid.

    // Act
    let codec = column_chunk_codec_for_test(&valid);
    let valid_views = decode_views(&valid);
    let corrupt_views = decode_views(&corrupt);
    // Assert
    assert_valid_twin(&valid, codec, valid_views);
    assert_one_byte_fault(
        &valid,
        &corrupt,
        34,
        corrupt_views,
        "non-zero trailing validity bits",
    );
}
