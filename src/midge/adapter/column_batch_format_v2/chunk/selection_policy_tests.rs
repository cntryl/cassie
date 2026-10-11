use serde_json::{json, Value};

use super::{decode, encode, encode_with_policy, Codec, EncodedChunk, LogicalType};

#[test]
fn should_apply_the_complete_chunk_thirty_two_byte_savings_boundary() {
    // Arrange
    let cases = [
        (23, 89, 58, 54, Codec::Plain),
        (24, 91, 59, 56, Codec::Constant),
        (25, 93, 60, 58, Codec::Constant),
    ];
    for (length, plain_len, constant_len, decoded_len, winner) in cases {
        let text = "abcdefghijklmnopqrstuvwxy"[..length].to_owned();
        let values = vec![json!(text), json!(text)];

        // Act
        let constant = encode_with_policy(LogicalType::Utf8, &values, Some(Codec::Constant))
            .expect("repeated strings must admit Constant");
        let plain = encode_with_policy(LogicalType::Utf8, &values, Some(Codec::Plain))
            .expect("Plain control must encode");
        let selected = encode(LogicalType::Utf8, &values).expect("automatic encoding must succeed");
        let repeated = encode(LogicalType::Utf8, &values).expect("repeated encoding must succeed");

        // Assert
        assert_chunk(
            &constant,
            LogicalType::Utf8,
            &values,
            Codec::Constant,
            constant_len,
            decoded_len,
            0,
        );
        assert_chunk(
            &plain,
            LogicalType::Utf8,
            &values,
            Codec::Plain,
            plain_len,
            decoded_len,
            0,
        );
        let expected = if winner == Codec::Plain {
            &plain
        } else {
            &constant
        };
        assert_chunk(
            &selected,
            LogicalType::Utf8,
            &values,
            winner,
            expected.bytes.len(),
            decoded_len,
            0,
        );
        assert_eq!(selected.bytes, expected.bytes);
        assert_eq!(selected.bytes, repeated.bytes);
    }
}

#[test]
fn should_apply_five_percent_to_the_complete_plain_chunk() {
    // Arrange
    let cases = [
        (5744, 5738, 800, 760, Codec::Constant),
        (5745, 5739, 801, 761, Codec::Plain),
    ];
    for (count, null_count, plain_len, constant_len, winner) in cases {
        let mut values = vec![Value::Null; count];
        values[..6].fill(json!(42));

        // Act
        let constant = encode_with_policy(LogicalType::Int64, &values, Some(Codec::Constant))
            .expect("six equal non-NULL integers must admit Constant");
        let plain = encode_with_policy(LogicalType::Int64, &values, Some(Codec::Plain))
            .expect("Plain control must encode");
        let selected =
            encode(LogicalType::Int64, &values).expect("automatic encoding must succeed");
        let repeated = encode(LogicalType::Int64, &values).expect("repeated encoding must succeed");

        // Assert
        assert_chunk(
            &constant,
            LogicalType::Int64,
            &values,
            Codec::Constant,
            constant_len,
            48,
            null_count,
        );
        assert_chunk(
            &plain,
            LogicalType::Int64,
            &values,
            Codec::Plain,
            plain_len,
            48,
            null_count,
        );
        let expected = if winner == Codec::Plain {
            &plain
        } else {
            &constant
        };
        assert_chunk(
            &selected,
            LogicalType::Int64,
            &values,
            winner,
            expected.bytes.len(),
            48,
            null_count,
        );
        assert_eq!(selected.bytes, expected.bytes);
        assert_eq!(selected.bytes, repeated.bytes);
    }
}

#[test]
fn should_prefer_rle_when_frame_of_reference_has_equal_complete_size() {
    // Arrange
    let mut values = vec![json!(0); 9];
    values.extend(vec![json!(255); 9]);

    // Act
    let rle = encode_with_policy(LogicalType::Int64, &values, Some(Codec::Rle))
        .expect("two integer runs must admit RLE");
    let frame = encode_with_policy(LogicalType::Int64, &values, Some(Codec::FrameOfReference))
        .expect("integer block must admit Frame-of-reference");
    let plain = encode_with_policy(LogicalType::Int64, &values, Some(Codec::Plain))
        .expect("Plain control must encode");
    let selected = encode(LogicalType::Int64, &values).expect("automatic encoding must succeed");
    let repeated = encode(LogicalType::Int64, &values).expect("repeated encoding must succeed");

    // Assert
    assert_chunk(&rle, LogicalType::Int64, &values, Codec::Rle, 73, 144, 0);
    assert_chunk(
        &frame,
        LogicalType::Int64,
        &values,
        Codec::FrameOfReference,
        73,
        144,
        0,
    );
    assert_chunk(
        &plain,
        LogicalType::Int64,
        &values,
        Codec::Plain,
        181,
        144,
        0,
    );
    assert_chunk(
        &selected,
        LogicalType::Int64,
        &values,
        Codec::Rle,
        73,
        144,
        0,
    );
    assert_eq!(selected.bytes, rle.bytes);
    assert_eq!(selected.bytes, repeated.bytes);
}

fn assert_chunk(
    chunk: &EncodedChunk,
    logical_type: LogicalType,
    values: &[Value],
    codec: Codec,
    complete_len: usize,
    decoded_len: usize,
    null_count: usize,
) {
    assert_eq!(chunk.codec, codec);
    assert_eq!(chunk.logical_type, logical_type);
    assert_eq!(chunk.value_count, values.len());
    assert_eq!(chunk.null_count, null_count);
    assert_eq!(chunk.bytes.len(), complete_len);
    assert_eq!(chunk.decoded_len, decoded_len);
    let decoded = decode(&chunk.bytes).expect("actual production encoding must decode");
    assert_eq!(decoded.codec, codec);
    assert_eq!(decoded.logical_type, logical_type);
    assert_eq!(decoded.encoded_len, complete_len);
    assert_eq!(decoded.decoded_len, decoded_len);
    assert_eq!(decoded.values, values);
    println!("codec={codec:?} type={logical_type:?} count={} nulls={null_count} complete_bytes={complete_len} decoded_bytes={decoded_len}", values.len());
}
