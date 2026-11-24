//! Storage-owned codecs for version 1 column batches.

mod alp;
mod alp_chunk;
pub mod binary;
mod bitpack;
mod boolean;
pub mod chunk;
mod fsst;
mod numeric;
mod row_ids;
mod selected;

pub use boolean::DecodedBooleanChunk;
pub use chunk::{Codec, DecodedChunk, EncodedChunk, LogicalType};
pub use numeric::{DecodedNumericChunk, NumericValues};

use crate::StorageCodecError;

pub fn encode_column_chunk(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, StorageCodecError> {
    chunk::encode(logical_type, values)
}

pub fn encode_plain_column_chunk(
    logical_type: LogicalType,
    values: &[serde_json::Value],
) -> Result<EncodedChunk, StorageCodecError> {
    chunk::encode_plain_for_test(logical_type, values)
}

pub fn encode_for_column_chunk(
    values: &[serde_json::Value],
) -> Result<EncodedChunk, StorageCodecError> {
    chunk::encode_for(values)
}

pub fn decode_column_chunk(bytes: &[u8]) -> Result<DecodedChunk, StorageCodecError> {
    chunk::decode(bytes)
}

pub fn decode_selected_column_chunk(
    bytes: &[u8],
    selection: &[bool],
) -> Result<DecodedChunk, StorageCodecError> {
    selected::decode(bytes, selection)
}

pub fn encode_row_ids(row_ids: &[String]) -> Result<Vec<u8>, StorageCodecError> {
    row_ids::encode(row_ids)
}

pub fn decode_row_ids(bytes: &[u8]) -> Result<Vec<String>, StorageCodecError> {
    row_ids::decode(bytes)
}

pub fn decode_numeric_column_chunk(
    bytes: &[u8],
    expected_rows: usize,
) -> Result<DecodedNumericChunk, StorageCodecError> {
    numeric::decode(bytes, expected_rows)
}

pub fn decode_boolean_column_chunk(
    bytes: &[u8],
    expected_rows: usize,
) -> Result<DecodedBooleanChunk, StorageCodecError> {
    boolean::decode(bytes, expected_rows)
}
