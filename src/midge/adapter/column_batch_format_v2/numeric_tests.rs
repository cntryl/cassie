use super::{
    decode_numeric_column_chunk, encode_column_chunk, encode_plain_column_chunk, Codec,
    LogicalType, NumericValues,
};

#[test]
fn should_decode_primitive_numeric_codecs_without_lane_objects() {
    // Arrange
    let cases = [
        (vec![serde_json::json!(7); 64], Codec::Constant),
        (
            (0..64)
                .map(|value| serde_json::json!(9_007_199_254_740_993_i64 + value))
                .collect(),
            Codec::FrameOfReference,
        ),
        (
            (0..2048)
                .map(|value| serde_json::json!(if value < 1024 { i64::MIN } else { i64::MAX }))
                .collect(),
            Codec::Rle,
        ),
        (
            (0..2048)
                .map(|value| serde_json::json!([i64::MIN, 0, i64::MAX][value % 3]))
                .collect(),
            Codec::Dictionary,
        ),
    ];
    // Act
    for (values, codec) in cases {
        let encoded = encode_column_chunk(LogicalType::Int64, &values).expect("encoded integers");
        let decoded = decode_numeric_column_chunk(&encoded.bytes, values.len())
            .expect("native numeric decoder");
        // Assert
        assert_eq!(encoded.codec, codec);
        let NumericValues::Integer(actual) = decoded.values else {
            panic!("integer owner")
        };
        assert_eq!(
            actual,
            values
                .iter()
                .map(serde_json::Value::as_i64)
                .collect::<Vec<_>>()
        );
    }
    let values = vec![
        serde_json::json!(i64::MAX),
        serde_json::Value::Null,
        serde_json::json!(i64::MIN),
    ];
    let encoded = encode_plain_column_chunk(LogicalType::Int64, &values).expect("plain");
    let decoded = decode_numeric_column_chunk(&encoded.bytes, values.len()).expect("plain numeric");
    let NumericValues::Integer(actual) = decoded.values else {
        panic!("integer owner")
    };
    assert_eq!(actual, vec![Some(i64::MAX), None, Some(i64::MIN)]);
}

#[test]
fn should_preserve_scaled_float_carriers_in_primitive_owners() {
    // Arrange
    let values = (0..2048)
        .map(|value| {
            if value % 17 == 0 {
                serde_json::Value::Null
            } else {
                serde_json::json!((f64::from(value) - 1024.0) / 4.0)
            }
        })
        .collect::<Vec<_>>();
    let encoded = encode_column_chunk(LogicalType::Float64, &values).expect("ALP input");
    // Act
    let decoded = decode_numeric_column_chunk(&encoded.bytes, values.len()).expect("ALP numeric");
    // Assert
    assert_eq!(encoded.codec, Codec::Alp);
    let NumericValues::Float(actual) = decoded.values else {
        panic!("float owner")
    };
    assert_eq!(
        actual,
        values
            .iter()
            .map(serde_json::Value::as_f64)
            .collect::<Vec<_>>()
    );
    let values = vec![
        serde_json::json!(-0.0),
        serde_json::json!(0.0),
        serde_json::Value::Null,
        serde_json::json!(f64::MAX),
    ];
    let encoded = encode_plain_column_chunk(LogicalType::Float64, &values).expect("plain FLOAT");
    let decoded = decode_numeric_column_chunk(&encoded.bytes, values.len()).expect("numeric FLOAT");
    let NumericValues::Float(actual) = decoded.values else {
        panic!("float owner")
    };
    assert!(actual[0].expect("negative zero").is_sign_negative());
    assert!(actual[1].expect("positive zero").is_sign_positive());
    assert_eq!(actual[2], None);
    assert_eq!(actual[3], Some(f64::MAX));
}

#[test]
fn should_reject_invalid_numeric_frames_before_decoding_lanes() {
    // Arrange
    let encoded = encode_plain_column_chunk(
        LogicalType::Int64,
        &[serde_json::json!(1), serde_json::Value::Null],
    )
    .expect("plain source");
    // Act
    let wrong_domain = decode_numeric_column_chunk(&encoded.bytes, 1);
    let truncated = decode_numeric_column_chunk(&encoded.bytes[..encoded.bytes.len() - 1], 2);
    let mut invalid_validity = encoded.bytes.clone();
    invalid_validity[34] ^= 1;
    let invalid = decode_numeric_column_chunk(&invalid_validity, 2);
    // Assert
    assert!(wrong_domain.is_err());
    assert!(truncated.is_err());
    assert!(invalid.is_err());
}

#[test]
fn should_reject_nonempty_numeric_dictionary_for_an_empty_domain() {
    // Arrange
    let mut bytes = encode_plain_column_chunk(LogicalType::Int64, &[])
        .expect("empty source")
        .bytes;
    bytes[7] = Codec::Dictionary as u8;
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&8_u32.to_le_bytes());
    payload.extend_from_slice(&7_i64.to_le_bytes());
    payload.extend_from_slice(&0_u32.to_le_bytes());
    bytes[22..26].copy_from_slice(
        &u32::try_from(payload.len())
            .expect("payload length")
            .to_le_bytes(),
    );
    bytes.extend(payload);
    // Act
    let decoded = decode_numeric_column_chunk(&bytes, 0);
    // Assert
    assert!(
        decoded.is_err(),
        "empty dictionaries cannot retain unused entries"
    );
}
