//! `CHAR(n)` (`bpchar`) value semantics.
//!
//! Trailing blanks are insignificant in a blank-padded character value, so
//! `'x'` and `'x  '` are the same `CHAR(3)` value. Cassie stores the value
//! without its trailing blanks, which makes equality, UNIQUE reservations and
//! index keys agree on that one canonical spelling.

/// Returns `value` without trailing blanks: the canonical stored form of a
/// `CHAR(n)` value.
#[must_use]
pub fn canonical_char_text(value: &str) -> &str {
    value.trim_end_matches(' ')
}

#[cfg(test)]
mod tests {
    use super::canonical_char_text;

    #[test]
    fn should_drop_only_trailing_blanks() {
        // Arrange
        let values = ["x  ", "  x", "x", "   ", "x\t "];

        // Act
        let canonical = values.map(canonical_char_text);

        // Assert
        assert_eq!(canonical, ["x", "  x", "x", "", "x\t"]);
    }
}
