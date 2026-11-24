//! Cassie persisted key framing backed by the published LexKey encoder.

use cntryl_lexkey::{Encoder, LexKey};

/// Encodes one Cassie family key using the established root and layout marker.
pub fn encode_key(family: &[u8], components: &[&[u8]]) -> Vec<u8> {
    encode(family, components, false)
}

/// Encodes a Cassie family prefix that can select all child keys.
pub fn encode_prefix(family: &[u8], components: &[&[u8]]) -> Vec<u8> {
    encode(family, components, true)
}

/// Encodes an unsigned numeric key component with the established LexKey form.
pub fn encode_u64_component(value: u64) -> Vec<u8> {
    let mut encoder = Encoder::with_capacity(9);
    encoder.encode_u64_into(value);
    encoder.into_vec()
}

/// Encodes the key identifying a persisted enum descriptor.
pub fn enum_type_key(canonical_name: &str) -> Vec<u8> {
    encode_key(b"enum-type", &[canonical_name.as_bytes()])
}

/// Encodes the prefix selecting persisted enum descriptors.
pub fn enum_type_prefix() -> Vec<u8> {
    encode_prefix(b"enum-type", &[])
}

/// Appends one scalar index value using Cassie's order-preserving value tags.
pub fn append_scalar_value(
    key: &mut Vec<u8>,
    value: &serde_json::Value,
) -> Result<(), crate::StorageCodecError> {
    let mut encoder = Encoder::with_capacity(10);
    match value {
        serde_json::Value::Null => key.extend_from_slice(&[0x10, LexKey::SEPARATOR]),
        serde_json::Value::Bool(value) => {
            key.push(0x20);
            encoder.encode_u8_into(u8::from(*value));
            key.extend_from_slice(encoder.as_slice());
            key.push(LexKey::SEPARATOR);
        }
        serde_json::Value::Number(number) => {
            if let Some(integer) = number
                .as_i64()
                .or_else(|| number.as_u64().and_then(|value| i64::try_from(value).ok()))
            {
                key.push(0x30);
                encoder.encode_i64_into(integer);
                key.extend_from_slice(encoder.as_slice());
                key.push(LexKey::SEPARATOR);
            } else if let Some(float) = number.as_f64() {
                if !float.is_finite() {
                    return Err(crate::StorageCodecError::UnsupportedValue(
                        "non-finite scalar index values are not supported".to_owned(),
                    ));
                }
                key.push(0x40);
                // PostgreSQL treats `-0` and `0` as equal.
                encoder.encode_f64_into(if float == 0.0 { 0.0 } else { float });
                key.extend_from_slice(encoder.as_slice());
                key.push(LexKey::SEPARATOR);
            } else {
                return Err(crate::StorageCodecError::UnsupportedValue(
                    "scalar index number is not representable".to_owned(),
                ));
            }
        }
        serde_json::Value::String(value) => {
            key.push(0x50);
            append_terminated_component(key, value.as_bytes());
        }
        serde_json::Value::Array(_) => {
            key.push(0x60);
            append_json_composite(key, value)?;
        }
        serde_json::Value::Object(_) => {
            key.push(0x70);
            append_json_composite(key, value)?;
        }
    }
    Ok(())
}

fn append_json_composite(
    key: &mut Vec<u8>,
    value: &serde_json::Value,
) -> Result<(), crate::StorageCodecError> {
    let canonical_value = canonicalize_json_object_keys(value);
    let canonical = serde_json::to_vec(&canonical_value).map_err(|error| {
        crate::StorageCodecError::InvalidEncoding(format!(
            "failed to encode JSON index key: {error}"
        ))
    })?;
    append_terminated_component(key, &canonical);
    Ok(())
}

fn canonicalize_json_object_keys(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.iter().map(canonicalize_json_object_keys).collect())
        }
        serde_json::Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);

            let mut canonical = serde_json::Map::new();
            for (key, value) in entries {
                canonical.insert(key.clone(), canonicalize_json_object_keys(value));
            }
            serde_json::Value::Object(canonical)
        }
        value => value.clone(),
    }
}

fn append_terminated_component(key: &mut Vec<u8>, bytes: &[u8]) {
    for byte in bytes {
        match *byte {
            LexKey::SEPARATOR => {
                key.push(LexKey::SEPARATOR);
                key.push(LexKey::END_MARKER);
            }
            value => key.push(value),
        }
    }
    key.push(LexKey::SEPARATOR);
}

fn encode(family: &[u8], components: &[&[u8]], trailing_separator: bool) -> Vec<u8> {
    const ROOT: &[u8] = b"cassie";
    const BASELINE: &[u8] = b"\x01";

    let mut parts = Vec::with_capacity(3 + components.len());
    parts.push(ROOT);
    parts.push(BASELINE);
    parts.push(family);
    parts.extend_from_slice(components);

    let capacity = parts.iter().map(|part| part.len()).sum::<usize>() + parts.len();
    let mut encoder = Encoder::with_capacity(capacity);
    encoder.encode_composite_into_buf(&parts);
    if trailing_separator {
        encoder.push_separator();
    }
    encoder.into_vec()
}

#[cfg(test)]
mod tests {
    use super::{encode_key, encode_prefix};
    use cntryl_lexkey::Encoder;

    #[test]
    fn should_preserve_existing_composite_key_bytes() {
        // Arrange
        let relation_id = 7_u64.to_be_bytes();
        let parts: [&[u8]; 5] = [b"cassie", b"\x01", b"row", &relation_id, b"id-1"];
        let mut expected = Encoder::with_capacity(32);
        expected.encode_composite_into_buf(&parts);

        // Act
        let actual = encode_key(b"row", &[&7_u64.to_be_bytes(), b"id-1"]);

        // Assert
        assert_eq!(actual, expected.into_vec());
    }

    #[test]
    fn should_preserve_existing_composite_prefix_bytes() {
        // Arrange
        let relation_id = 7_u64.to_be_bytes();
        let parts: [&[u8]; 4] = [b"cassie", b"\x01", b"row", &relation_id];
        let mut expected = Encoder::with_capacity(32);
        expected.encode_composite_into_buf(&parts);
        expected.push_separator();

        // Act
        let actual = encode_prefix(b"row", &[&7_u64.to_be_bytes()]);

        // Assert
        assert_eq!(actual, expected.into_vec());
    }
}
