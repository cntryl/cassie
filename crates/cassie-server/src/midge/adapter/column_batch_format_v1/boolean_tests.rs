//! Plain Boolean native decode preserves the existing CBC1 acceptance boundary.
use super::{
    decode_boolean_column_chunk, decode_column_chunk, encode_plain_column_chunk, Codec, LogicalType,
};

fn values(rows: usize, all_null: bool) -> Vec<serde_json::Value> {
    (0..rows)
        .map(|lane| {
            if all_null || lane % 3 == 1 {
                serde_json::Value::Null
            } else {
                serde_json::json!(lane % 3 == 0)
            }
        })
        .collect()
}
#[test]
fn should_decode_plain_boolean_lanes_through_sparse_validity() {
    // Arrange
    let lengths = [0, 1, 7, 8, 9, 63, 64, 65, 1023, 1024, 1025];
    for rows in lengths {
        for all_null in [false, true] {
            let expected = values(rows, all_null);
            let encoded =
                encode_plain_column_chunk(LogicalType::Boolean, &expected).expect("Plain Boolean");
            // Act
            let decoded =
                decode_boolean_column_chunk(&encoded.bytes, rows).expect("native Boolean");
            let generic = decode_column_chunk(&encoded.bytes).expect("existing decoder");
            // Assert
            assert_eq!(
                decoded.values,
                expected
                    .iter()
                    .map(serde_json::Value::as_bool)
                    .collect::<Vec<_>>()
            );
            assert_eq!(generic.values, expected);
            assert_eq!(
                decoded.null_count,
                expected.iter().filter(|value| value.is_null()).count()
            );
            assert_eq!(decoded.logical_type, LogicalType::Boolean);
            assert_eq!(decoded.codec, Codec::Plain);
            assert_eq!(decoded.encoded_len, encoded.bytes.len());
            assert_eq!(decoded.decoded_len, encoded.decoded_len);
        }
    }
}
#[test]
fn should_preserve_boolean_payload_padding_acceptance() {
    // Arrange
    let mut bytes = encode_plain_column_chunk(
        LogicalType::Boolean,
        &[
            serde_json::json!(true),
            serde_json::Value::Null,
            serde_json::json!(false),
        ],
    )
    .expect("Plain Boolean")
    .bytes;
    *bytes.last_mut().expect("payload") |= 0b1111_1100;
    // Act
    let native =
        decode_boolean_column_chunk(&bytes, 3).expect("unused payload bits remain accepted");
    let generic = decode_column_chunk(&bytes).expect("existing acceptance");
    // Assert
    assert_eq!(native.values, vec![Some(true), None, Some(false)]);
    assert_eq!(
        generic.values,
        vec![
            serde_json::json!(true),
            serde_json::Value::Null,
            serde_json::json!(false)
        ]
    );
}
#[test]
fn should_reject_nonzero_boolean_validity_padding() {
    // Arrange
    let mut bytes = encode_plain_column_chunk(LogicalType::Boolean, &[serde_json::json!(true)])
        .expect("Plain Boolean")
        .bytes;
    bytes[34] |= 0b1000_0000;
    // Act
    let native = decode_boolean_column_chunk(&bytes, 1);
    let generic = decode_column_chunk(&bytes);
    // Assert
    assert!(native.is_err());
    assert!(generic.is_err());
}
#[test]
fn should_reject_boolean_domain_mismatch_before_native_lane_construction() {
    // Arrange
    let bytes = encode_plain_column_chunk(LogicalType::Boolean, &values(9, false))
        .expect("Plain Boolean")
        .bytes;
    // Act
    let error = decode_boolean_column_chunk(&bytes, 8)
        .err()
        .expect("wrong domain");
    // Assert
    assert!(error.to_string().contains("domain mismatch"), "{error}");
}
#[test]
fn should_reject_malformed_plain_boolean_framing() {
    // Arrange
    let original = encode_plain_column_chunk(LogicalType::Boolean, &values(9, false))
        .expect("Plain Boolean")
        .bytes;
    let mut cases = vec![original[..original.len() - 1].to_vec()];
    for (offset, value) in [(4, 2), (6, 1), (8, 1), (14, 255), (18, 255), (22, 255)] {
        let mut bytes = original.clone();
        bytes[offset] = value;
        cases.push(bytes);
    }
    // Act
    let results = cases
        .iter()
        .map(|bytes| decode_boolean_column_chunk(bytes, 9).is_err())
        .collect::<Vec<_>>();
    // Assert
    assert!(results.into_iter().all(|rejected| rejected));
}

/// Small valid non-Plain fixtures, independent of the automatic savings threshold.
pub(crate) fn nonplain(codec: Codec) -> Vec<u8> {
    let mut bytes = encode_plain_column_chunk(
        LogicalType::Boolean,
        &[
            serde_json::json!(false),
            serde_json::json!(true),
            serde_json::json!(false),
        ],
    )
    .expect("framing")
    .bytes;
    let mut payload = Vec::new();
    match codec {
        Codec::Constant => payload.push(1),
        Codec::Rle => {
            payload.extend_from_slice(&3_u32.to_le_bytes());
            for value in [0, 1, 0] {
                payload.extend_from_slice(&1_u32.to_le_bytes());
                payload.extend_from_slice(&1_u32.to_le_bytes());
                payload.push(value);
            }
        }
        Codec::Dictionary => {
            payload.extend_from_slice(&2_u32.to_le_bytes());
            for value in [0, 1] {
                payload.extend_from_slice(&1_u32.to_le_bytes());
                payload.push(value);
            }
            payload.extend_from_slice(&1_u32.to_le_bytes()); // one index block
            payload.push(3);
            payload.extend_from_slice(&0_u64.to_le_bytes());
            payload.push(1);
            payload.extend_from_slice(&1_u32.to_le_bytes());
            payload.push(0b0000_0010);
        }
        _ => panic!("non-Plain fixture"),
    }
    bytes[7] = codec as u8;
    bytes[22..26].copy_from_slice(
        &u32::try_from(payload.len())
            .expect("small payload")
            .to_le_bytes(),
    );
    bytes.truncate(35);
    bytes.extend(payload);
    bytes
}
#[test]
fn should_keep_other_boolean_codecs_out_of_the_plain_native_decoder() {
    // Arrange
    let cases = [Codec::Constant, Codec::Rle, Codec::Dictionary];
    for codec in cases {
        let bytes = nonplain(codec);
        // Act
        let native = decode_boolean_column_chunk(&bytes, 3);
        let generic = decode_column_chunk(&bytes).expect("existing non-Plain codec");
        // Assert
        assert!(native.is_err());
        let expected = if codec == Codec::Constant {
            vec![true, true, true]
        } else {
            vec![false, true, false]
        };
        assert_eq!(
            generic.values,
            expected
                .into_iter()
                .map(serde_json::Value::Bool)
                .collect::<Vec<_>>()
        );
    }
}
