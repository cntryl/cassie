//! A literal-aware token scan of stored SQL text.
//!
//! Stored view and projection definitions are SQL text. Rewrites that must
//! change a name inside that text need to find names only where SQL reads them
//! as names, so this scan classifies every string form (plain, `E'..'` with
//! backslash escapes, `U&'..'`, `B`/`X`/`N` strings, dollar quotes with or
//! without a tag), quoted identifiers and comments, and never reports names
//! inside them. Comments and whitespace are dropped from the token list.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A bare identifier, keyword or number.
    Word,
    /// A double-quoted identifier.
    Quoted,
    /// Any string literal.
    Literal,
    /// One punctuation byte.
    Punct,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    pub(super) fn text<'a>(&self, sql: &'a str) -> &'a str {
        &sql[self.start..self.end]
    }

    pub(super) fn is_punct(&self, sql: &str, byte: u8) -> bool {
        self.kind == Kind::Punct && sql.as_bytes()[self.start] == byte
    }

    pub(super) fn is_keyword(&self, sql: &str, keyword: &str) -> bool {
        self.kind == Kind::Word && self.text(sql).eq_ignore_ascii_case(keyword)
    }

    /// The folded identifier this token names: bare words fold to lower case,
    /// quoted identifiers keep their exact spelling. `None` for other tokens
    /// and for words that start with a digit.
    pub(super) fn identifier(&self, sql: &str) -> Option<String> {
        match self.kind {
            Kind::Word if !sql.as_bytes()[self.start].is_ascii_digit() => {
                Some(self.text(sql).to_ascii_lowercase())
            }
            Kind::Quoted => Some(
                sql[self.start + 1..self.end.saturating_sub(1).max(self.start + 1)]
                    .replace("\"\"", "\""),
            ),
            _ => None,
        }
    }
}

const fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// The offset just past the run opened at `start`, where `quote` closes it and
/// a doubled quote is an escaped quote; `backslash` also lets `\` escape the
/// next byte (`E'..'`).
fn quoted_run_end(bytes: &[u8], start: usize, quote: u8, backslash: bool) -> usize {
    let mut index = start + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if backslash && byte == b'\\' {
            index += 2;
            continue;
        }
        if byte == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
                continue;
            }
            return index + 1;
        }
        index += 1;
    }
    bytes.len()
}

/// The end of the dollar-quoted string opened at `start`, when a tag opener
/// (`$$` or `$tag$`) starts there.
pub(super) fn dollar_quote_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    if bytes.get(index).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    while bytes
        .get(index)
        .copied()
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80)
    {
        index += 1;
    }
    if bytes.get(index) != Some(&b'$') {
        return None;
    }
    let tag = &bytes[start..=index];
    let body = index + 1;
    let close = bytes[body.min(bytes.len())..]
        .windows(tag.len())
        .position(|window| window == tag);
    Some(close.map_or(bytes.len(), |offset| body + offset + tag.len()))
}

fn block_comment_end(bytes: &[u8], start: usize) -> usize {
    let mut depth = 0_usize;
    let mut index = start;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"/*") {
            depth += 1;
            index += 2;
        } else if bytes[index..].starts_with(b"*/") {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            index += 1;
        }
    }
    bytes.len()
}

/// Splits `sql` into tokens, skipping whitespace and comments.
pub(super) fn tokenize(sql: &str) -> Vec<Token> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        let kind = if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        } else if bytes[index..].starts_with(b"--") {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        } else if bytes[index..].starts_with(b"/*") {
            index = block_comment_end(bytes, index);
            continue;
        } else if byte == b'\'' {
            index = quoted_run_end(bytes, index, b'\'', false);
            Kind::Literal
        } else if byte == b'"' {
            index = quoted_run_end(bytes, index, b'"', false);
            Kind::Quoted
        } else if byte == b'$' {
            if let Some(end) = dollar_quote_end(bytes, index) {
                index = end;
                Kind::Literal
            } else {
                index += 1;
                Kind::Punct
            }
        } else if is_identifier_byte(byte) {
            while bytes.get(index).copied().is_some_and(is_identifier_byte) {
                index += 1;
            }
            let prefix = &bytes[start..index];
            if prefix.len() == 1 && bytes.get(index) == Some(&b'\'') {
                let escapes = matches!(prefix[0], b'e' | b'E');
                if escapes || matches!(prefix[0], b'b' | b'B' | b'x' | b'X' | b'n' | b'N') {
                    index = quoted_run_end(bytes, index, b'\'', escapes);
                    tokens.push(Token {
                        kind: Kind::Literal,
                        start,
                        end: index,
                    });
                    continue;
                }
            }
            if matches!(prefix, b"u" | b"U") && bytes[index..].starts_with(b"&'") {
                index = quoted_run_end(bytes, index + 1, b'\'', false);
                Kind::Literal
            } else {
                Kind::Word
            }
        } else {
            index += 1;
            Kind::Punct
        };
        tokens.push(Token {
            kind,
            start,
            end: index,
        });
    }
    tokens
}

/// A dotted name found in the token stream: the indexes of its identifier
/// tokens (a trailing `*` is not part of it).
pub(super) struct Chain {
    pub parts: Vec<usize>,
}

/// Every maximal `a.b.c` identifier chain that does not continue a preceding
/// `.` (so `(expr).field` selections are not chains of their own).
pub(super) fn chains(sql: &str, tokens: &[Token]) -> Vec<Chain> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let follows_dot = index > 0 && tokens[index - 1].is_punct(sql, b'.');
        if follows_dot || tokens[index].identifier(sql).is_none() {
            index += 1;
            continue;
        }
        let mut parts = vec![index];
        let mut next = index + 1;
        while next + 1 < tokens.len()
            && tokens[next].is_punct(sql, b'.')
            && tokens[next + 1].identifier(sql).is_some()
        {
            parts.push(next + 1);
            next += 2;
        }
        index = next;
        found.push(Chain { parts });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{tokenize, Kind};

    fn literals(sql: &str) -> Vec<String> {
        tokenize(sql)
            .iter()
            .filter(|token| token.kind == Kind::Literal)
            .map(|token| token.text(sql).to_owned())
            .collect()
    }

    #[test]
    fn should_keep_an_escape_string_with_an_escaped_quote_as_one_literal() {
        // Arrange
        let sql = r"SELECT E'\'' AS q, 'x' AS s";

        // Act
        let found = literals(sql);

        // Assert
        assert_eq!(found, vec![r"E'\''".to_owned(), "'x'".to_owned()]);
    }

    #[test]
    fn should_keep_dollar_quoted_strings_as_one_literal() {
        // Arrange
        let sql = "SELECT $$ a::dom $$, $t$ ' $t$, $1";

        // Act
        let found = literals(sql);

        // Assert
        assert_eq!(
            found,
            vec!["$$ a::dom $$".to_owned(), "$t$ ' $t$".to_owned()]
        );
    }

    #[test]
    fn should_drop_line_comments_with_nested_block_comments() {
        // Arrange
        let sql = "a -- x::dom\n/* y /* z */ w */ b";

        // Act
        let words = tokenize(sql)
            .iter()
            .map(|token| token.text(sql).to_owned())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(words, vec!["a".to_owned(), "b".to_owned()]);
    }
}
