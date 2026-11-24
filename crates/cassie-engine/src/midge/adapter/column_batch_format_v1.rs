//! Engine compatibility facade for storage-owned column codecs.

use crate::app::CassieError;

pub(crate) use cassie_storage::column_batch_format_v1::{
    Codec, DecodedChunk, DecodedNumericChunk, EncodedChunk, LogicalType, NumericValues,
};

pub(crate) use cassie_storage::column_batch_format_v1::DecodedBooleanChunk;

pub(crate) fn encode_column_chunk(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, CassieError> {
    cassie_storage::column_batch_format_v1::encode_column_chunk(logical_type, values)
        .map_err(CassieError::from)
}

pub(crate) fn encode_plain_column_chunk(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, CassieError> {
    cassie_storage::column_batch_format_v1::encode_plain_column_chunk(logical_type, values)
        .map_err(CassieError::from)
}

pub(crate) fn decode_column_chunk(bytes: &[u8]) -> Result<DecodedChunk, CassieError> {
    cassie_storage::column_batch_format_v1::decode_column_chunk(bytes).map_err(CassieError::from)
}

pub(crate) fn decode_selected_column_chunk(
    bytes: &[u8],
    selection: &[bool],
) -> Result<DecodedChunk, CassieError> {
    cassie_storage::column_batch_format_v1::decode_selected_column_chunk(bytes, selection)
        .map_err(CassieError::from)
}

pub(crate) fn encode_row_ids(row_ids: &[String]) -> Result<Vec<u8>, CassieError> {
    cassie_storage::column_batch_format_v1::encode_row_ids(row_ids).map_err(CassieError::from)
}

pub(crate) fn decode_row_ids(bytes: &[u8]) -> Result<Vec<String>, CassieError> {
    cassie_storage::column_batch_format_v1::decode_row_ids(bytes).map_err(CassieError::from)
}

pub(crate) fn decode_numeric_column_chunk(
    bytes: &[u8],
    expected_rows: usize,
) -> Result<DecodedNumericChunk, CassieError> {
    cassie_storage::column_batch_format_v1::decode_numeric_column_chunk(bytes, expected_rows)
        .map_err(CassieError::from)
}

pub(crate) fn decode_boolean_column_chunk(
    bytes: &[u8],
    expected_rows: usize,
) -> Result<DecodedBooleanChunk, CassieError> {
    cassie_storage::column_batch_format_v1::decode_boolean_column_chunk(bytes, expected_rows)
        .map_err(CassieError::from)
}

pub(crate) use cassie_storage::column_batch_format_v1::{
    checksum_hex, decode_manifest, encode_manifest, has_current_header, summary_checksum,
    MANIFEST_FORMAT_VERSION, MANIFEST_SUMMARY_VERSION,
};

#[doc(hidden)]
pub fn encode_column_chunk_for_test(
    logical_type: &str,
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let logical_type = LogicalType::from_name(logical_type)?;
    encode_column_chunk(logical_type, values).map(|encoded| encoded.bytes)
}

#[doc(hidden)]
pub fn encode_for_column_chunk_for_test(
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    cassie_storage::column_batch_format_v1::encode_for_column_chunk(values)
        .map(|encoded| encoded.bytes)
        .map_err(CassieError::from)
}

#[doc(hidden)]
pub fn encode_plain_column_chunk_for_test(
    logical_type: &str,
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let logical_type = LogicalType::from_name(logical_type)?;
    encode_plain_column_chunk(logical_type, values).map(|encoded| encoded.bytes)
}

#[doc(hidden)]
pub fn decode_column_chunk_for_test(bytes: &[u8]) -> Result<Vec<serde_json::Value>, CassieError> {
    decode_column_chunk(bytes).map(|decoded| decoded.values)
}

#[doc(hidden)]
pub fn decode_selected_column_chunk_for_test(
    bytes: &[u8],
    selection: &[bool],
) -> Result<Vec<serde_json::Value>, CassieError> {
    decode_selected_column_chunk(bytes, selection).map(|decoded| decoded.values)
}

#[doc(hidden)]
pub fn column_chunk_codec_for_test(bytes: &[u8]) -> Result<&'static str, CassieError> {
    decode_column_chunk(bytes).map(|decoded| decoded.codec.name())
}

#[doc(hidden)]
pub fn encode_row_id_chunk_for_test(row_ids: &[String]) -> Result<Vec<u8>, CassieError> {
    encode_row_ids(row_ids)
}

#[doc(hidden)]
pub fn decode_row_id_chunk_for_test(bytes: &[u8]) -> Result<Vec<String>, CassieError> {
    decode_row_ids(bytes)
}

#[doc(hidden)]
pub fn encode_column_batch_manifest_for_test(
    metadata: &crate::catalog::ColumnBatchMetadata,
) -> Result<Vec<u8>, CassieError> {
    cassie_storage::column_batch_format_v1::encode_column_batch_manifest_for_test(metadata)
        .map_err(CassieError::from)
}

#[doc(hidden)]
pub fn decode_column_batch_manifest_for_test(
    bytes: &[u8],
) -> Result<crate::catalog::ColumnBatchMetadata, CassieError> {
    cassie_storage::column_batch_format_v1::decode_column_batch_manifest_for_test(bytes)
        .map_err(CassieError::from)
}

#[cfg(test)]
mod numeric_tests;

#[cfg(test)]
pub(crate) mod boolean_tests;

#[cfg(test)]
#[path = "column_batch_boolean_source_tests.rs"]
mod boolean_source_tests;

#[cfg(test)]
mod storage_error_tests {
    use super::decode_column_chunk;

    #[test]
    fn should_keep_malformed_chunk_sqlstate_after_storage_conversion() {
        // Arrange
        let malformed = b"bad";

        // Act
        let error = decode_column_chunk(malformed).expect_err("malformed bytes");

        // Assert
        assert_eq!(error.sql_state(), "42601");
        assert_eq!(error.to_string(), "truncated column-batch record");
    }
}
