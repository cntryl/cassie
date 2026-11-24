//! Cassie persisted key framing backed by the published LexKey encoder.

use cntryl_lexkey::Encoder;

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
