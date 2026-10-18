//! PostgreSQL's SQL identifier boundary, separate from stored metadata names.
pub(super) const IDENTIFIER_BYTES: usize = 63;

pub(super) fn prefix(input: &str) -> &str {
    let mut end = input.len().min(IDENTIFIER_BYTES);
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
}

/// Decode doubled quotes, retaining only the native UTF-8-safe prefix while
/// scanning the complete token for its closing delimiter.
pub(super) fn quoted(body: &str, limit: usize) -> Option<(String, usize)> {
    let mut value = String::with_capacity(body.len().min(limit));
    let mut truncated = false;
    let mut chars = body.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        if character == '"' {
            if matches!(chars.peek(), Some((_, '"'))) {
                chars.next();
            } else {
                return Some((value, index + 1));
            }
        }
        if !truncated && value.len() + character.len_utf8() <= limit {
            value.push(character);
        } else {
            truncated = true;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::sql::ast::IdentifierPath;

    #[test]
    fn should_preserve_stored_metadata_given_long_names_when_sql_names_are_truncated() {
        // Arrange
        let input = format!("{}éz", "a".repeat(62));
        // Act
        let stored = IdentifierPath::from_field_name(&input);
        let parsed = IdentifierPath::parse(&format!("\"{input}\"")).expect("SQL identifier");
        // Assert
        assert_eq!(stored.local_component().unwrap().value(), input);
        assert_eq!(parsed.local_component().unwrap().value(), "a".repeat(62));
    }

    #[test]
    fn should_validate_closing_quote_given_truncated_prefix_when_decoding_identifiers() {
        // Arrange
        let body = "a".repeat(4096);
        // Act
        let unterminated = super::quoted(&body, super::IDENTIFIER_BYTES);
        let terminated = super::quoted(&format!("{body}\""), super::IDENTIFIER_BYTES);
        // Assert
        assert!(unterminated.is_none());
        assert_eq!(terminated, Some(("a".repeat(63), body.len() + 1)));
    }
    #[test]
    fn should_preserve_internal_qualifiers_given_generated_names_when_sql_tokens_are_bounded() {
        // Arrange
        let name = format!("__cassie_relation_alias_{}", "a".repeat(128));
        // Act
        let internal = crate::sql::ColumnIdentifierPath::parse(&name).expect("internal qualifier");
        let sql = crate::sql::ColumnIdentifierPath::parse_sql(&name).expect("SQL token");
        // Assert
        assert_eq!(internal.final_name(), name);
        assert_eq!(sql.final_name(), &name[..63]);
    }
}
