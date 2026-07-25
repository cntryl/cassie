mod binary;
mod bitpack;
mod chunk;
mod fsst;
mod manifest;
mod row_ids;
mod selected;

use crate::app::CassieError;

pub(crate) use chunk::{DecodedChunk, EncodedChunk, LogicalType};

pub(crate) fn encode_column_chunk(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, CassieError> {
    chunk::encode(logical_type, values)
}

pub(crate) fn encode_plain_column_chunk(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, CassieError> {
    chunk::encode_plain_for_test(logical_type, values)
}

pub(crate) fn decode_column_chunk(bytes: &[u8]) -> Result<DecodedChunk, CassieError> {
    chunk::decode(bytes)
}

pub(crate) fn decode_selected_column_chunk(
    bytes: &[u8],
    selection: &[bool],
) -> Result<DecodedChunk, CassieError> {
    selected::decode(bytes, selection)
}

pub(crate) fn encode_row_ids(row_ids: &[String]) -> Result<Vec<u8>, CassieError> {
    row_ids::encode(row_ids)
}

pub(crate) fn decode_row_ids(bytes: &[u8]) -> Result<Vec<String>, CassieError> {
    row_ids::decode(bytes)
}

pub(crate) use manifest::{
    checksum_hex, decode as decode_manifest, encode as encode_manifest, has_current_header,
    summary_checksum, FORMAT_VERSION as MANIFEST_FORMAT_VERSION,
    SUMMARY_VERSION as MANIFEST_SUMMARY_VERSION,
};

#[doc(hidden)]
pub fn encode_column_chunk_for_test(
    logical_type: &str,
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let logical_type = LogicalType::from_name(logical_type)?;
    chunk::encode(logical_type, values).map(|encoded| encoded.bytes)
}

#[doc(hidden)]
pub fn encode_for_column_chunk_for_test(
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    chunk::encode_for(values).map(|encoded| encoded.bytes)
}

#[doc(hidden)]
pub fn encode_plain_column_chunk_for_test(
    logical_type: &str,
    values: &[serde_json::Value],
) -> Result<Vec<u8>, CassieError> {
    let logical_type = LogicalType::from_name(logical_type)?;
    chunk::encode_plain_for_test(logical_type, values).map(|encoded| encoded.bytes)
}

#[doc(hidden)]
pub fn decode_column_chunk_for_test(bytes: &[u8]) -> Result<Vec<serde_json::Value>, CassieError> {
    chunk::decode(bytes).map(|decoded| decoded.values)
}

#[doc(hidden)]
pub fn decode_selected_column_chunk_for_test(
    bytes: &[u8],
    selection: &[bool],
) -> Result<Vec<serde_json::Value>, CassieError> {
    selected::decode(bytes, selection).map(|decoded| decoded.values)
}

#[doc(hidden)]
pub fn column_chunk_codec_for_test(bytes: &[u8]) -> Result<&'static str, CassieError> {
    chunk::decode(bytes).map(|decoded| decoded.codec.name())
}

#[doc(hidden)]
pub fn encode_row_id_chunk_for_test(row_ids: &[String]) -> Result<Vec<u8>, CassieError> {
    row_ids::encode(row_ids)
}

#[doc(hidden)]
pub fn decode_row_id_chunk_for_test(bytes: &[u8]) -> Result<Vec<String>, CassieError> {
    row_ids::decode(bytes)
}

#[doc(hidden)]
pub fn encode_column_batch_manifest_for_test(
    metadata: &crate::catalog::ColumnBatchMetadata,
) -> Result<Vec<u8>, CassieError> {
    manifest::encode(metadata)
}

#[doc(hidden)]
pub fn decode_column_batch_manifest_for_test(
    bytes: &[u8],
) -> Result<crate::catalog::ColumnBatchMetadata, CassieError> {
    manifest::decode(bytes)
}
