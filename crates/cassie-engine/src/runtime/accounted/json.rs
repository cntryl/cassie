#[cfg(test)]
#[path = "json/controlled_parse.rs"]
pub(crate) mod controlled_parse;
#[path = "json/tree_estimate.rs"]
pub(crate) mod tree_estimate;

use cassie_core::request::accounted::json;

use crate::app::CassieError;

pub(crate) use json::{JSON_DECODE_EXPANSION_FACTOR, JSON_MAP_ENTRY_BYTES};

/// Estimates an already-owned JSON value, including unused String and Vec capacity.
///
/// # Errors
///
/// Returns a resource-limit error if the size estimate overflows.
#[doc(hidden)]
pub fn retained_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    Ok(json::retained_bytes(value)?)
}

/// Estimates fresh parser/reconstruction output whose empty maps were never populated.
pub(crate) fn retained_decoded_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    Ok(json::retained_decoded_bytes(value)?)
}

/// # Errors
///
/// Returns a resource-limit error if the size estimate overflows.
#[doc(hidden)]
pub fn serialized_decode_bytes(raw_bytes: usize) -> Result<usize, CassieError> {
    Ok(json::serialized_decode_bytes(raw_bytes)?)
}
