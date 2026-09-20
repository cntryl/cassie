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
