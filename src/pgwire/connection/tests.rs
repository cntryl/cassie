use super::*;
use crate::executor::{ColumnMeta, QueryResult};
use crate::types::{DataType, Value};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::AsyncWrite;

#[derive(Default)]
struct CountingWrite {
    bytes: Vec<u8>,
    flushes: usize,
}

impl AsyncWrite for CountingWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.bytes.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.flushes += 1;
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[test]
fn should_flush_pgwire_simple_query_result_once_for_multiple_rows() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let result = QueryResult {
        columns: vec![ColumnMeta::from_data_type("id", &DataType::Text)],
        rows: vec![
            vec![Value::String("doc-1".to_string())],
            vec![Value::String("doc-2".to_string())],
        ],
        command: "SELECT".to_string(),
    };

    runtime.block_on(async {
        let mut writer = CountingWrite::default();

        // Act
        write_simple_query_result(&mut writer, result)
            .await
            .expect("write simple query result");

        // Assert
        assert_eq!(writer.flushes, 1);
        assert_eq!(writer.bytes[0], b'T');
        assert!(writer.bytes.contains(&b'D'));
        assert!(writer.bytes.contains(&b'C'));
    });
}

#[test]
fn should_reject_binary_float8_for_integer_beyond_exact_range() {
    // Arrange
    let inexact = Value::Int64(9_007_199_254_740_993);
    let exact = Value::Int64(-9_007_199_254_740_992);

    // Act
    let inexact_result = super::codecs::value_to_binary(inexact, 701);
    let exact_result = super::codecs::value_to_binary(exact, 701);

    // Assert
    let error = inexact_result.expect_err("inexact int8 must not be rounded to float8");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        exact_result.expect("exact int8 encodes as float8"),
        (-9_007_199_254_740_992.0_f64).to_be_bytes().to_vec()
    );
}

#[test]
fn should_reject_binary_float8_for_i64_max() {
    // Arrange
    let saturating = Value::Int64(i64::MAX);
    let minimum = Value::Int64(i64::MIN);

    // Act
    let saturating_result = super::codecs::value_to_binary(saturating, 701);
    let minimum_result = super::codecs::value_to_binary(minimum, 701);

    // Assert
    let error = saturating_result.expect_err("i64::MAX has no exact float8 representation");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        minimum_result.expect("i64::MIN is an exact power of two"),
        (-9_223_372_036_854_775_808.0_f64).to_be_bytes().to_vec()
    );
}

#[test]
fn should_preserve_json_string_documents_in_binary_array_elements() {
    // Arrange
    let value = Value::Json(serde_json::json!(["hi"]));
    let expected = [
        0, 0, 0, 1, // One SQL array dimension.
        0, 0, 0, 0, // No SQL NULL element.
        0, 0, 0, 114, // Existing JSON element OID.
        0, 0, 0, 1, // One element.
        0, 0, 0, 1, // Lower bound one.
        0, 0, 0, 4, // JSON document byte length, including quotes.
        b'"', b'h', b'i', b'"',
    ];

    // Act
    let encoded = super::codecs::value_to_binary(value.clone(), 34_114);
    let decoded = super::codecs::binary_to_value(&expected, 34_114);

    // Assert
    assert_eq!(encoded.expect("encode JSON string array element"), expected);
    assert_eq!(decoded.expect("decode JSON string array element"), value);
}

#[test]
fn should_preserve_nested_json_documents_in_binary_array_elements() {
    // Arrange
    let value = Value::Json(serde_json::json!([[1, 2]]));
    let expected = [
        0, 0, 0, 1, // One SQL array dimension, containing one JSON document.
        0, 0, 0, 0, // No SQL NULL element.
        0, 0, 0, 114, // Existing JSON element OID.
        0, 0, 0, 1, // One element.
        0, 0, 0, 1, // Lower bound one.
        0, 0, 0, 5, // JSON document byte length.
        b'[', b'1', b',', b'2', b']',
    ];

    // Act
    let encoded = super::codecs::value_to_binary(value.clone(), 34_114);
    let decoded = super::codecs::binary_to_value(&expected, 34_114);

    // Assert
    assert_eq!(
        encoded.expect("encode nested JSON array document"),
        expected
    );
    assert_eq!(decoded.expect("decode nested JSON array document"), value);
}
#[test]
fn should_encode_declared_vectors_from_json_returning_arrays() {
    // Arrange
    let values = [
        Value::Json(serde_json::json!([1.5, -2.0])),
        Value::Json(serde_json::json!([
            f64::from(f32::MAX),
            f64::from(f32::MIN)
        ])),
    ];
    let expected = [
        vec![0, 2, 0, 0, 63, 192, 0, 0, 192, 0, 0, 0],
        vec![0, 2, 0, 0, 127, 127, 255, 255, 255, 127, 255, 255],
    ];

    // Act
    let results = values.map(|value| super::codecs::value_to_binary(value, 100_002));

    // Assert
    for (result, expected) in results.into_iter().zip(expected) {
        assert_eq!(result.expect("declared vector RETURNING carrier"), expected);
    }
}

#[test]
fn should_reject_invalid_json_returning_vector_components() {
    // Arrange
    let values = [
        Value::Json(serde_json::json!([])),
        Value::Json(serde_json::json!([1.0])),
        Value::Json(serde_json::json!([1.0, 2.0, 3.0])),
        Value::Json(serde_json::json!([null, 2.0])),
        Value::Json(serde_json::json!(["1", 2.0])),
        Value::Json(serde_json::json!([true, 2.0])),
        Value::Json(serde_json::json!([[1], 2.0])),
        Value::Json(serde_json::json!([f64::MAX, 2.0])),
        Value::String("[1.0]".to_string()),
    ];

    // Act
    let results = values.map(|value| super::codecs::value_to_binary(value, 100_002));

    // Assert
    for result in results {
        assert_eq!(
            result
                .expect_err("invalid declared vector representation")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
#[test]
fn should_roundtrip_maximum_supported_binary_vector_width() {
    // Arrange
    let vector = crate::types::Vector::new(vec![0.25; 32_767]);
    let expected = Value::Vector(vector.clone());
    let expected_header = [127, 255, 0, 0];

    // Act
    let bytes = super::codecs::value_to_binary(Value::Vector(vector), 132_767)
        .expect("maximum registered scalar vector width");
    let decoded = super::codecs::binary_to_value(&bytes, 132_767)
        .expect("decode maximum registered scalar vector width");

    // Assert
    assert_eq!(bytes.len(), 131_072);
    assert_eq!(bytes[..4], expected_header);
    assert!(bytes[4..]
        .as_chunks::<4>()
        .0
        .iter()
        .all(|component| *component == [62, 128, 0, 0]));
    assert_eq!(decoded, expected);
}

#[test]
fn should_encode_declared_vectors_from_text_parameter_strings() {
    // Arrange
    let cases = [
        ("[1.5,-2.0]", vec![0, 2, 0, 0, 63, 192, 0, 0, 192, 0, 0, 0]),
        (
            " [ 1.5 , -2 ] ",
            vec![0, 2, 0, 0, 63, 192, 0, 0, 192, 0, 0, 0],
        ),
        (
            "[3.4028234663852886e38,-3.4028234663852886e38]",
            vec![0, 2, 0, 0, 127, 127, 255, 255, 255, 127, 255, 255],
        ),
        (
            "[0.1,-0.0]",
            vec![0, 2, 0, 0, 61, 204, 204, 205, 128, 0, 0, 0],
        ),
    ];

    // Act
    let results = cases.map(|(text, expected)| {
        (
            text,
            super::codecs::value_to_binary(Value::String(text.to_string()), 100_002),
            expected,
        )
    });

    // Assert
    for (text, result, expected) in results {
        assert_eq!(
            result.unwrap_or_else(|error| panic!("declared vector {text}: {error}")),
            expected,
            "declared vector {text}"
        );
    }
}

#[test]
fn should_reject_invalid_declared_vector_text_parameter_strings() {
    // Arrange
    let values = [
        "null",
        "[]",
        "[1]",
        "[1,2,3]",
        "[null,2]",
        "[\"1\",2]",
        "[true,2]",
        "[[1],2]",
        "[3.4028236e38,2]",
        "[NaN,2]",
        "[1,2] trailing",
    ];

    // Act
    let results =
        values.map(|text| super::codecs::value_to_binary(Value::String(text.to_string()), 100_002));

    // Assert
    for result in results {
        assert_eq!(
            result.expect_err("invalid declared vector text").kind(),
            io::ErrorKind::InvalidData
        );
    }
}

// Focused mechanism probe: parse borrowed fixed-array tokens with the same
// RawValue deserializer used by the production vector SeqAccess visitor.
#[test]
fn should_borrow_vector_numeric_tokens_from_the_original_text() {
    // Arrange
    let text = "[3.4028234663852886e38,-3.4028234663852886e38]";
    let expected = ["3.4028234663852886e38", "-3.4028234663852886e38"];
    let offsets = [1, 2 + expected[0].len()];

    // Act
    let tokens = serde_json::from_str::<[&serde_json::value::RawValue; 2]>(text)
        .expect("borrowed numeric JSON tokens");
    let encoded = super::codecs::value_to_binary(Value::String(text.to_string()), 100_002);

    // Assert
    for ((token, expected), offset) in tokens.into_iter().zip(expected).zip(offsets) {
        assert_eq!(token.get(), expected);
        assert!(std::ptr::eq(
            token.get().as_ptr(),
            text.as_ptr().wrapping_add(offset)
        ));
    }
    assert_eq!(
        encoded.expect("strict finite VECTOR boundary"),
        [0, 2, 0, 0, 127, 127, 255, 255, 255, 127, 255, 255]
    );
}

#[test]
fn should_reject_oversized_vector_text_sequences_before_returning_output() {
    // Arrange
    let long_sequence = format!("[1,2,{}]", "3,".repeat(16_384).trim_end_matches(','));
    let inputs = [
        long_sequence,
        "[1,2,{\"nested\":[true,null]}]".to_string(),
        "[1,2] trailing".to_string(),
        "[1,{\"not_a_number\":2}]".to_string(),
    ];

    // Act
    let outputs = inputs.map(|text| super::codecs::value_to_binary(Value::String(text), 100_002));

    // Assert
    for output in outputs {
        assert_eq!(
            output
                .expect_err("invalid vector sequence has no encoded output")
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
