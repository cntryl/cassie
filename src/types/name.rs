//! PostgreSQL `name` input and wire boundaries.

pub const MAX_NAME_BYTES: usize = 63;

/// Truncates a name at the longest UTF-8 boundary that fits in PostgreSQL's
/// 63-byte payload limit.
#[must_use]
pub fn truncate(text: &str) -> String {
    if text.len() <= MAX_NAME_BYTES {
        return text.to_owned();
    }
    let mut end = MAX_NAME_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Parses the text input form, which truncates overlong values.
#[must_use]
pub fn parse_text(text: &str) -> String {
    truncate(text)
}

/// Parses the binary receive form, which rejects values at 64 bytes or more.
///
/// # Errors
///
/// Returns an error for invalid UTF-8 or an overlong name.
pub fn parse_binary(bytes: &[u8]) -> Result<String, &'static str> {
    if bytes.len() > MAX_NAME_BYTES {
        return Err("name value exceeds 63 bytes");
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "name value is not valid UTF-8")?;
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{parse_binary, parse_text, MAX_NAME_BYTES};

    #[test]
    fn should_truncate_name_text_at_a_utf8_boundary() {
        // Arrange
        let value = format!("{}é", "a".repeat(MAX_NAME_BYTES - 1));

        // Act
        let parsed = parse_text(&value);

        // Assert
        assert_eq!(parsed, "a".repeat(MAX_NAME_BYTES - 1));
    }

    #[test]
    fn should_reject_overlong_binary_name_values() {
        // Arrange
        let value = vec![b'a'; MAX_NAME_BYTES + 1];

        // Act
        let parsed = parse_binary(&value);

        // Assert
        assert_eq!(parsed, Err("name value exceeds 63 bytes"));
    }
}
