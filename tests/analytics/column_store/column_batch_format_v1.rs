use cassie::catalog::ColumnBatchMetadata;
use cassie::midge::adapter::{
    column_chunk_codec_for_test, decode_column_batch_manifest_for_test,
    decode_column_chunk_for_test, decode_row_id_chunk_for_test,
    decode_selected_column_chunk_for_test, encode_column_batch_manifest_for_test,
    encode_column_chunk_for_test, encode_plain_column_chunk_for_test, encode_row_id_chunk_for_test,
};
use sha2::{Digest, Sha256};

fn encode(logical_type: &str, values: &[serde_json::Value]) -> Vec<u8> {
    encode_column_chunk_for_test(logical_type, values).expect("encode column chunk")
}

#[test]
fn should_preserve_signed_zero_bits_in_constant_codec_candidates() {
    // Arrange
    for logical_type in ["float", "vector", "json"] {
        for negative_first in [false, true] {
            let mut scalars = vec![serde_json::json!(0.0); 41];
            let position = if negative_first { 0 } else { 40 };
            scalars[position] = serde_json::json!(-0.0);
            let values = if logical_type == "float" {
                scalars
            } else {
                scalars
                    .into_iter()
                    .map(|value| serde_json::json!([value]))
                    .collect()
            };

            // Act
            let encoded = encode(logical_type, &values);
            let decoded = decode_column_chunk_for_test(&encoded).expect("decode chunk");

            // Assert
            for (expected, actual) in values.iter().zip(&decoded) {
                let (expected, actual) = if logical_type == "float" {
                    (expected, actual)
                } else {
                    (&expected[0], &actual[0])
                };
                assert_eq!(
                    actual.as_f64().expect("float").to_bits(),
                    expected.as_f64().expect("float").to_bits()
                );
            }
        }
    }
}

#[test]
fn should_emit_platform_independent_little_endian_plain_integer_bytes() {
    // Arrange
    let values = [serde_json::json!(1), serde_json::json!(-2)];

    // Act
    let encoded = encode("bigint", &values);

    // Assert
    assert_eq!(
        encoded,
        vec![
            b'C', b'B', b'C', b'1', 1, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 16, 0, 0,
            0, 16, 0, 0, 0, 0, 0, 0, 0, 3, 1, 0, 0, 0, 0, 0, 0, 0, 254, 255, 255, 255, 255, 255,
            255, 255,
        ]
    );
}

#[test]
fn should_roundtrip_sparse_validity_with_signed_extremes() {
    // Arrange
    let values = [
        serde_json::json!(i64::MIN),
        serde_json::Value::Null,
        serde_json::json!(0),
        serde_json::Value::Null,
        serde_json::json!(i64::MAX),
    ];

    // Act
    let encoded = encode("bigint", &values);
    let decoded = decode_column_chunk_for_test(&encoded).expect("decode integer chunk");

    // Assert
    assert_eq!(decoded, values);
}

#[test]
fn should_select_each_typed_codec_deterministically() {
    // Arrange
    let constant = vec![serde_json::json!(7); 256];
    let rle = (0..8)
        .flat_map(|value| std::iter::repeat_n(serde_json::json!(format!("run-{value}")), 64))
        .collect::<Vec<_>>();
    let dictionary = (0..512)
        .map(|position| serde_json::json!(format!("status-{}", position % 4)))
        .collect::<Vec<_>>();
    let frame_of_reference = (0..512)
        .map(|value| serde_json::json!(10_000_i64 + i64::from(value)))
        .collect::<Vec<_>>();

    // Act
    let constant_codec =
        column_chunk_codec_for_test(&encode("bigint", &constant)).expect("constant codec");
    let rle_codec = column_chunk_codec_for_test(&encode("text", &rle)).expect("rle codec");
    let dictionary_codec =
        column_chunk_codec_for_test(&encode("text", &dictionary)).expect("dictionary codec");
    let for_codec =
        column_chunk_codec_for_test(&encode("bigint", &frame_of_reference)).expect("for codec");

    // Assert
    assert_eq!(constant_codec, "constant");
    assert_eq!(rle_codec, "rle");
    assert_eq!(dictionary_codec, "dictionary");
    assert_eq!(for_codec, "frame_of_reference");
}

#[test]
fn should_roundtrip_zero_width_frame_of_reference_blocks() {
    // Arrange
    let values = (0..256).map(|_| serde_json::json!(42)).collect::<Vec<_>>();

    // Act
    let encoded = cassie::midge::adapter::encode_for_column_chunk_for_test(&values)
        .expect("encode forced FOR chunk");
    let decoded = decode_column_chunk_for_test(&encoded).expect("decode zero-width FOR");

    // Assert
    assert_eq!(decoded, values);
}

#[test]
fn should_limit_float_codec_selection_to_supported_encodings() {
    // Arrange
    let values = (0..512)
        .map(|value| serde_json::json!(f64::from(value) / 10.0))
        .collect::<Vec<_>>();

    // Act
    let encoded = encode("float", &values);
    let codec = column_chunk_codec_for_test(&encoded).expect("float codec");
    let decoded = decode_column_chunk_for_test(&encoded).expect("decode floats");

    // Assert
    assert!(matches!(codec, "plain" | "constant" | "rle" | "alp"));
    assert_eq!(decoded, values);
}

#[test]
fn should_roundtrip_repeated_utf8_with_fsst() {
    // Arrange
    let values = (0..512)
        .map(|position| {
            serde_json::json!(format!(
                "tenant-{}/event-{}-payload-{}",
                position % 32,
                position,
                position % 8
            ))
        })
        .collect::<Vec<_>>();

    // Act
    let encoded = encode("text", &values);
    let codec = column_chunk_codec_for_test(&encoded).expect("text codec");
    let decoded = decode_column_chunk_for_test(&encoded).expect("decode FSST text");

    // Assert
    assert_eq!(codec, "fsst");
    assert_eq!(decoded, values);
}

#[test]
fn should_decode_only_selected_fsst_positions() {
    // Arrange
    let values = (0..512)
        .map(|position| {
            if position % 73 == 0 {
                serde_json::Value::Null
            } else {
                serde_json::json!(format!(
                    "tenant-{}/event-{position}-payload-{}",
                    position % 32,
                    position % 8
                ))
            }
        })
        .collect::<Vec<_>>();
    let encoded = encode("text", &values);
    let selection = (0..values.len())
        .map(|position| position % 64 == 0)
        .collect::<Vec<_>>();

    // Act
    let decoded = decode_selected_column_chunk_for_test(&encoded, &selection)
        .expect("decode selected FSST values");

    // Assert
    assert_eq!(
        column_chunk_codec_for_test(&encoded).expect("text codec"),
        "fsst"
    );
    for (position, value) in decoded.iter().enumerate() {
        if selection[position] {
            assert_eq!(value, &values[position]);
        } else {
            assert!(value.is_null());
        }
    }
}

#[test]
fn should_emit_cross_architecture_stable_fsst_bytes() {
    // Arrange
    let values = (0..512)
        .map(|position| {
            serde_json::json!(format!(
                "tenant-{}/event-{position}-payload-{}",
                position % 32,
                position % 8
            ))
        })
        .collect::<Vec<_>>();

    // Act
    let encoded = encode("text", &values);
    let digest = Sha256::digest(&encoded);

    // Assert
    assert_eq!(
        column_chunk_codec_for_test(&encoded).expect("text codec"),
        "fsst"
    );
    assert_eq!(
        digest.as_slice(),
        &[
            130, 73, 146, 31, 239, 92, 148, 170, 170, 57, 26, 167, 220, 142, 5, 156, 93, 130, 76,
            23, 50, 91, 205, 167, 195, 39, 41, 137, 242, 13, 6, 248,
        ]
    );
    assert_eq!(encoded.len(), 15_193);
}

#[test]
fn should_roundtrip_alp_finite_values_bit_exactly() {
    // Arrange
    let values = (0..512)
        .map(|position| serde_json::json!((f64::from(position) - 256.0) / 100.0))
        .collect::<Vec<_>>();

    // Act
    let encoded = encode("float", &values);
    let codec = column_chunk_codec_for_test(&encoded).expect("float codec");
    let decoded = decode_column_chunk_for_test(&encoded).expect("decode ALP floats");

    // Assert
    assert_eq!(codec, "alp");
    assert_eq!(decoded.len(), values.len());
    for (actual, expected) in decoded.iter().zip(values.iter()) {
        assert_eq!(
            actual.as_f64().expect("decoded float").to_bits(),
            expected.as_f64().expect("input float").to_bits()
        );
    }
}

#[test]
fn should_emit_cross_architecture_stable_alp_bytes() {
    // Arrange
    let values = (0..512)
        .map(|position| serde_json::json!((f64::from(position) - 256.0) / 100.0))
        .collect::<Vec<_>>();

    // Act
    let encoded = encode("float", &values);
    let digest = Sha256::digest(&encoded);

    // Assert
    assert_eq!(
        column_chunk_codec_for_test(&encoded).expect("float codec"),
        "alp"
    );
    assert_eq!(
        digest.as_slice(),
        &[
            56, 198, 97, 234, 119, 208, 132, 51, 250, 32, 125, 61, 168, 122, 108, 33, 3, 238, 31,
            197, 9, 91, 137, 171, 123, 185, 129, 87, 57, 66, 242, 26,
        ]
    );
    assert_eq!(encoded.len(), 608);
}

#[test]
fn should_reject_alp_values_reconstructed_outside_i64() {
    // Arrange
    let values = (0..128)
        .map(|position| serde_json::json!(f64::from(position) / 100.0))
        .collect::<Vec<_>>();
    let mut encoded = encode("float", &values);
    assert_eq!(
        column_chunk_codec_for_test(&encoded).expect("float codec"),
        "alp"
    );
    let validity_len =
        u32::from_le_bytes(encoded[18..22].try_into().expect("validity length bytes"));
    let payload_offset = 34 + usize::try_from(validity_len).expect("validity length");
    let base_offset = payload_offset + 8;
    encoded[base_offset..base_offset + 8].copy_from_slice(&i64::MAX.to_le_bytes());

    // Act
    let decoded = decode_column_chunk_for_test(&encoded);

    // Assert
    assert!(decoded.is_err());
}

#[test]
fn should_keep_signed_zero_out_of_alp() {
    // Arrange
    let negative_zero = serde_json::Number::from_f64(-0.0).expect("negative zero");
    let values = vec![
        serde_json::Value::Number(negative_zero),
        serde_json::json!(1.25),
    ];

    // Act
    let encoded = encode("float", &values);
    let codec = column_chunk_codec_for_test(&encoded).expect("float codec");
    let decoded = decode_column_chunk_for_test(&encoded).expect("decode floats");

    // Assert
    assert_ne!(codec, "alp");
    assert_eq!(
        decoded[0].as_f64().expect("decoded float").to_bits(),
        (-0.0_f64).to_bits()
    );
}

#[test]
fn should_fall_back_to_plain_when_savings_do_not_clear_the_threshold() {
    // Arrange
    let values = (0..64)
        .map(|value| serde_json::json!(format!("unique-{value:04}")))
        .collect::<Vec<_>>();

    // Act
    let encoded = encode("text", &values);
    let codec = column_chunk_codec_for_test(&encoded).expect("plain codec");

    // Assert
    assert_eq!(codec, "plain");
}

#[test]
fn should_reduce_representative_compressible_bytes_by_at_least_twenty_five_percent() {
    // Arrange
    let values = (0..1_024)
        .map(|position| serde_json::json!(format!("status-{}", position % 4)))
        .collect::<Vec<_>>();

    // Act
    let selected = encode("text", &values);
    let plain = encode_plain_column_chunk_for_test("text", &values).expect("encode plain baseline");

    // Assert
    assert!(selected.len() <= plain.len());
    assert!(
        selected.len().saturating_mul(4) <= plain.len().saturating_mul(3),
        "selected={} plain={}",
        selected.len(),
        plain.len()
    );
}

#[test]
fn should_validate_selected_dictionary_payload_before_partial_decode() {
    // Arrange
    let values = (0..512)
        .map(|position| serde_json::json!(format!("long-dictionary-value-{}", position % 4)))
        .collect::<Vec<_>>();
    let encoded = encode("text", &values);
    let selection = (0..values.len())
        .map(|position| matches!(position, 17 | 255 | 511))
        .collect::<Vec<_>>();

    // Act
    let decoded = decode_selected_column_chunk_for_test(&encoded, &selection)
        .expect("decode selected dictionary values");

    // Assert
    assert_eq!(decoded.len(), values.len());
    for (position, value) in decoded.iter().enumerate() {
        if selection[position] {
            assert_eq!(value, &values[position]);
        } else {
            assert!(value.is_null());
        }
    }
    for boundary in 0..encoded.len() {
        assert!(
            decode_selected_column_chunk_for_test(&encoded[..boundary], &selection).is_err(),
            "boundary {boundary} unexpectedly decoded"
        );
    }
}

#[test]
fn should_reject_every_truncated_column_chunk_boundary() {
    // Arrange
    let encoded = encode(
        "bigint",
        &(0..300)
            .map(|value| serde_json::json!(value))
            .collect::<Vec<_>>(),
    );

    // Act
    let outcomes = (0..encoded.len())
        .map(|boundary| decode_column_chunk_for_test(&encoded[..boundary]))
        .collect::<Vec<_>>();

    // Assert
    for (boundary, outcome) in outcomes.into_iter().enumerate() {
        assert!(outcome.is_err(), "boundary {boundary} unexpectedly decoded");
    }
}

#[test]
fn should_reject_unsupported_or_oversized_column_chunks() {
    // Arrange
    let encoded = encode("bigint", &[serde_json::json!(1), serde_json::json!(2)]);
    let mut unknown_codec = encoded.clone();
    unknown_codec[7] = u8::MAX;
    let mut excessive_count = encoded;
    excessive_count[10..14].copy_from_slice(&u32::MAX.to_le_bytes());

    // Act
    let codec_error = decode_column_chunk_for_test(&unknown_codec);
    let count_error = decode_column_chunk_for_test(&excessive_count);

    // Assert
    assert!(codec_error.is_err());
    assert!(count_error.is_err());
}

#[test]
fn should_validate_bounded_row_id_chunk_roundtrips() {
    // Arrange
    let row_ids = vec![
        "row-0001".to_string(),
        "row-0002".to_string(),
        "row-0100".to_string(),
    ];

    // Act
    let encoded = encode_row_id_chunk_for_test(&row_ids).expect("encode row IDs");
    let decoded = decode_row_id_chunk_for_test(&encoded).expect("decode row IDs");

    // Assert
    assert_eq!(&encoded[..4], b"CBR1");
    assert_eq!(decoded, row_ids);
    for boundary in 0..encoded.len() {
        assert!(
            decode_row_id_chunk_for_test(&encoded[..boundary]).is_err(),
            "boundary {boundary} unexpectedly decoded"
        );
    }
}

#[test]
fn should_validate_manifest_roundtrips_against_truncation_or_checksum_failure() {
    // Arrange
    let metadata = ColumnBatchMetadata {
        metadata_format_version: 1,
        summary_format_version: 1,
        manifest_revision: 9,
        next_segment_id: 0,
        collection: "postgres.public.events".to_string(),
        index_name: "events_column_idx".to_string(),
        schema_version: 7,
        built_generation: 11,
        source_row_count: 0,
        fields: vec!["tenant".to_string(), "amount".to_string()],
        segment_size: 128,
        segments: Vec::new(),
    };

    // Act
    let encoded =
        encode_column_batch_manifest_for_test(&metadata).expect("encode canonical manifest");
    let decoded =
        decode_column_batch_manifest_for_test(&encoded).expect("decode canonical manifest");

    // Assert
    assert_eq!(decoded, metadata);
    assert!(encoded.starts_with(b"CBM1"));
    for boundary in 0..encoded.len() {
        assert!(
            decode_column_batch_manifest_for_test(&encoded[..boundary]).is_err(),
            "accepted manifest truncation at boundary {boundary}"
        );
    }
    for position in [0, encoded.len() / 2, encoded.len() - 1] {
        let mut corrupt = encoded.clone();
        corrupt[position] ^= 0x40;
        assert!(
            decode_column_batch_manifest_for_test(&corrupt).is_err(),
            "accepted corrupt manifest byte at {position}"
        );
    }
    let mut trailing = encoded;
    trailing.push(0);
    assert!(decode_column_batch_manifest_for_test(&trailing).is_err());
}
