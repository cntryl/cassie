use super::SqlError;

pub(crate) const MAX_SQL_BYTES: usize = 1024 * 1024;
const MAX_SQL_TOKENS: usize = 100_000;
const MAX_SQL_NESTING: usize = 128;
const MAX_BLOCK_COMMENT_NESTING: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanState {
    Normal,
    SingleQuoted,
    DoubleQuoted,
    LineComment,
    BlockComment(usize),
}

pub(crate) fn preflight_sql(sql: &str) -> Result<(), SqlError> {
    if sql.len() > MAX_SQL_BYTES {
        return Err(SqlError::resource_limit(format!(
            "SQL text exceeds {MAX_SQL_BYTES} bytes"
        )));
    }

    let bytes = sql.as_bytes();
    let mut state = ScanState::Normal;
    let mut index = 0_usize;
    let mut nesting = 0_usize;
    let mut tokens = 0_usize;
    let mut in_word = false;
    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        match state {
            ScanState::Normal => scan_normal_byte(
                byte,
                next,
                &mut state,
                &mut index,
                &mut nesting,
                &mut tokens,
                &mut in_word,
            )?,
            ScanState::SingleQuoted => {
                if byte == b'\'' {
                    if next == Some(b'\'') {
                        index += 1;
                    } else {
                        state = ScanState::Normal;
                    }
                } else if byte == b'\\' && next.is_some() {
                    index += 1;
                }
            }
            ScanState::DoubleQuoted => {
                if byte == b'"' {
                    if next == Some(b'"') {
                        index += 1;
                    } else {
                        state = ScanState::Normal;
                    }
                }
            }
            ScanState::LineComment => {
                if matches!(byte, b'\n' | b'\r') {
                    state = ScanState::Normal;
                }
            }
            ScanState::BlockComment(depth) => match (byte, next) {
                (b'/', Some(b'*')) => {
                    let next_depth = depth.saturating_add(1);
                    if next_depth > MAX_BLOCK_COMMENT_NESTING {
                        return Err(SqlError::resource_limit(format!(
                            "nested block comment depth exceeds {MAX_BLOCK_COMMENT_NESTING}"
                        )));
                    }
                    state = ScanState::BlockComment(next_depth);
                    index += 1;
                }
                (b'*', Some(b'/')) => {
                    state = if depth == 1 {
                        ScanState::Normal
                    } else {
                        ScanState::BlockComment(depth - 1)
                    };
                    index += 1;
                }
                _ => {}
            },
        }
        index += 1;
    }
    finish_word(&mut in_word, &mut tokens)?;
    Ok(())
}

fn scan_normal_byte(
    byte: u8,
    next: Option<u8>,
    state: &mut ScanState,
    index: &mut usize,
    nesting: &mut usize,
    tokens: &mut usize,
    in_word: &mut bool,
) -> Result<(), SqlError> {
    match (byte, next) {
        (b'\'', _) => {
            finish_word(in_word, tokens)?;
            count_token(tokens)?;
            *state = ScanState::SingleQuoted;
        }
        (b'"', _) => {
            finish_word(in_word, tokens)?;
            count_token(tokens)?;
            *state = ScanState::DoubleQuoted;
        }
        (b'-', Some(b'-')) => {
            finish_word(in_word, tokens)?;
            *state = ScanState::LineComment;
            *index += 1;
        }
        (b'/', Some(b'*')) => {
            finish_word(in_word, tokens)?;
            *state = ScanState::BlockComment(1);
            *index += 1;
        }
        (b'(', _) => {
            finish_word(in_word, tokens)?;
            count_token(tokens)?;
            *nesting = nesting.saturating_add(1);
            if *nesting > MAX_SQL_NESTING {
                return Err(SqlError::resource_limit(format!(
                    "SQL nesting exceeds {MAX_SQL_NESTING}"
                )));
            }
        }
        (b')', _) => {
            finish_word(in_word, tokens)?;
            count_token(tokens)?;
            *nesting = nesting.saturating_sub(1);
        }
        (value, _) if value.is_ascii_whitespace() => finish_word(in_word, tokens)?,
        (value, _) if is_word_byte(value) => *in_word = true,
        _ => {
            finish_word(in_word, tokens)?;
            count_token(tokens)?;
        }
    }
    Ok(())
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$' | b'.')
}

fn finish_word(in_word: &mut bool, tokens: &mut usize) -> Result<(), SqlError> {
    if *in_word {
        count_token(tokens)?;
        *in_word = false;
    }
    Ok(())
}

fn count_token(tokens: &mut usize) -> Result<(), SqlError> {
    *tokens = tokens.saturating_add(1);
    if *tokens > MAX_SQL_TOKENS {
        return Err(SqlError::resource_limit(format!(
            "SQL lexical token count exceeds {MAX_SQL_TOKENS}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::SqlErrorKind;

    #[test]
    fn should_reject_sql_text_over_one_mebibyte() {
        // Arrange
        let sql = "x".repeat(MAX_SQL_BYTES + 1);

        // Act
        let error = preflight_sql(&sql).expect_err("oversized SQL");

        // Assert
        assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
    }

    #[test]
    fn should_reject_excessive_parenthesis_and_comment_nesting() {
        // Arrange
        let parentheses = format!("{}1{}", "(".repeat(129), ")".repeat(129));
        let comments = format!("{}x{}", "/*".repeat(129), "*/".repeat(129));

        // Act
        let parenthesis_error = preflight_sql(&parentheses).expect_err("parenthesis limit");
        let comment_error = preflight_sql(&comments).expect_err("comment limit");

        // Assert
        assert_eq!(parenthesis_error.kind(), SqlErrorKind::ResourceLimit);
        assert_eq!(comment_error.kind(), SqlErrorKind::ResourceLimit);
    }

    #[test]
    fn should_reject_more_than_one_hundred_thousand_lexical_tokens() {
        // Arrange
        let sql = format!("{}x", "x,".repeat(MAX_SQL_TOKENS / 2));

        // Act
        let error = preflight_sql(&sql).expect_err("token limit");

        // Assert
        assert_eq!(error.kind(), SqlErrorKind::ResourceLimit);
        assert!(error.to_string().contains("token count"));
    }

    #[test]
    fn should_ignore_delimiters_inside_strings_and_comments() {
        // Arrange
        let sql = format!(
            "SELECT '{}', \"{}\" /* {} */",
            "(".repeat(256),
            ")".repeat(256),
            "(".repeat(256)
        );

        // Act
        let result = preflight_sql(&sql);

        // Assert
        assert!(result.is_ok());
    }
}
