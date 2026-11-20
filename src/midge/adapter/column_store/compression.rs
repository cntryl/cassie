//! Per-value compression of column-authoritative field payloads (#1357).
//!
//! A stored field value is the compact encoding `[type_tag][payload]`. When a
//! value is large and repetitive enough that LZ4 saves at least a quarter of
//! its bytes, the stored bytes become
//! `[COMPRESSED_TAG][original_len: u32 BE][lz4 block]`, where the block expands
//! to the original compact encoding. Every other value is stored unchanged, so
//! incompressible and small values pay no space and no read cost, and the only
//! extra write cost is one bounded compression attempt per value of at least
//! [`MIN_COMPRESSIBLE_BYTES`].
//!
//! The tag is outside the `TYPE_*` range, so a build that predates it reads a
//! compressed value as a type-tag mismatch (`XX001`) instead of wrong data.

use crate::app::CassieError;
use crate::midge::row_blob::{encode_compact_sql_null, encode_compact_value};
use crate::types::DataType;
use std::borrow::Cow;

/// First byte of a compressed field value; no `TYPE_*` tag uses it.
pub(crate) const COMPRESSED_TAG: u8 = 0x80;
/// Values shorter than this are never compressed.
pub(crate) const MIN_COMPRESSIBLE_BYTES: usize = 128;
/// Largest original value a compressed payload may declare.
pub(crate) const MAX_ORIGINAL_BYTES: usize = 64 * 1024 * 1024;
const HEADER_BYTES: usize = 1 + std::mem::size_of::<u32>();

/// Encodes one field value for storage, compressing it when that saves at
/// least a quarter of the compact bytes.
///
/// # Errors
///
/// Returns the compact encoder's error when the value does not fit the type.
pub(crate) fn encode_column_field(
    data_type: &DataType,
    value: Option<&serde_json::Value>,
    compress: bool,
) -> Result<Vec<u8>, CassieError> {
    let compact = match value {
        Some(value) => encode_compact_value(data_type, value)?,
        None => encode_compact_sql_null(),
    };
    if !compress || compact.len() < MIN_COMPRESSIBLE_BYTES || compact.len() > MAX_ORIGINAL_BYTES {
        return Ok(compact);
    }
    let Ok(original_len) = u32::try_from(compact.len()) else {
        return Ok(compact);
    };
    let block = lz4_flex::block::compress(&compact);
    let stored_len = HEADER_BYTES + block.len();
    // Keep the original unless the saving is at least a quarter.
    if stored_len.saturating_mul(4) > compact.len().saturating_mul(3) {
        return Ok(compact);
    }
    let mut stored = Vec::with_capacity(stored_len);
    stored.push(COMPRESSED_TAG);
    stored.extend_from_slice(&original_len.to_be_bytes());
    stored.extend_from_slice(&block);
    Ok(stored)
}

/// The compact encoding a stored field value expands to.
///
/// # Errors
///
/// Returns a parse error naming the defect when a compressed payload has a
/// bad header, an out-of-bound length, or does not expand to its declared size.
pub(crate) fn expand_column_field(raw: &[u8]) -> Result<Cow<'_, [u8]>, CassieError> {
    if raw.first() != Some(&COMPRESSED_TAG) {
        return Ok(Cow::Borrowed(raw));
    }
    let header = raw
        .get(1..HEADER_BYTES)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .ok_or_else(|| CassieError::Parse("truncated compressed field header".to_string()))?;
    let original_len = usize::try_from(u32::from_be_bytes(header))
        .map_err(|_| CassieError::Parse("compressed field length overflows".to_string()))?;
    if !(MIN_COMPRESSIBLE_BYTES..=MAX_ORIGINAL_BYTES).contains(&original_len) {
        return Err(CassieError::Parse(format!(
            "compressed field declares {original_len} original bytes outside the supported range"
        )));
    }
    let expanded =
        lz4_flex::block::decompress(&raw[HEADER_BYTES..], original_len).map_err(|error| {
            CassieError::Parse(format!("compressed field does not expand: {error}"))
        })?;
    if expanded.len() != original_len {
        return Err(CassieError::Parse(
            "compressed field expanded to a different length than declared".to_string(),
        ));
    }
    Ok(Cow::Owned(expanded))
}

/// The byte length a stored value occupies once expanded, without expanding it.
/// A malformed header reports the stored length; decoding then rejects it.
pub(crate) fn logical_len(raw: &[u8]) -> usize {
    if raw.first() != Some(&COMPRESSED_TAG) {
        return raw.len();
    }
    raw.get(1..HEADER_BYTES)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map_or(raw.len(), |header| {
            usize::try_from(u32::from_be_bytes(header))
                .unwrap_or(raw.len())
                .max(raw.len())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn should_round_trip_a_compressible_text_value_smaller_than_its_compact_form() {
        // Arrange
        let value = json!("abc".repeat(200));
        let compact = encode_compact_value(&DataType::Text, &value).expect("compact");

        // Act
        let stored = encode_column_field(&DataType::Text, Some(&value), true).expect("stored");
        let expanded = expand_column_field(&stored).expect("expanded");

        // Assert
        assert_eq!(stored.first(), Some(&COMPRESSED_TAG));
        assert!(stored.len() < compact.len() / 2);
        assert_eq!(expanded.as_ref(), compact.as_slice());
        assert_eq!(logical_len(&stored), compact.len());
    }

    #[test]
    fn should_keep_short_or_incompressible_values_unchanged() {
        // Arrange
        let short = json!("tenant-0001");
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let noisy = (0..512)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                char::from(b'a' + u8::try_from((state >> 33) % 26).expect("letter"))
            })
            .collect::<String>();
        let noisy = json!(noisy);

        // Act
        let short_stored = encode_column_field(&DataType::Text, Some(&short), true).expect("short");
        let noisy_stored = encode_column_field(&DataType::Text, Some(&noisy), true).expect("noisy");

        // Assert
        assert_eq!(
            short_stored,
            encode_compact_value(&DataType::Text, &short).expect("compact")
        );
        assert_eq!(
            noisy_stored,
            encode_compact_value(&DataType::Text, &noisy).expect("compact")
        );
    }

    #[test]
    fn should_reject_a_compressed_value_with_a_wrong_declared_length() {
        // Arrange
        let value = json!("abc".repeat(200));
        let mut stored = encode_column_field(&DataType::Text, Some(&value), true).expect("stored");
        stored[4] ^= 0x01;

        // Act
        let result = expand_column_field(&stored);

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn should_not_compress_when_disabled() {
        // Arrange
        let value = json!("abc".repeat(200));

        // Act
        let stored = encode_column_field(&DataType::Text, Some(&value), false).expect("stored");

        // Assert
        assert_ne!(stored.first(), Some(&COMPRESSED_TAG));
    }
}
