//! Lexical positions in a stored view definition (#827).
//!
//! A view is re-bound from its SQL text, so `ALTER TYPE ... RENAME` must edit
//! that text where it names the enum or one of its labels. This scanner only
//! finds string literals and identifiers with their byte ranges; deciding
//! which of them refer to the enum is the caller's job.

/// What a [`Token`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenKind {
    /// A single-quoted string literal; `text` is its unescaped value.
    Literal,
    /// An identifier; `text` is folded to lower case unless it was quoted.
    Identifier,
}

/// One literal or identifier of a SQL text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Token {
    pub kind: TokenKind,
    /// Byte range of the token, quotes included.
    pub start: usize,
    pub end: usize,
    /// Start of the dotted name this identifier ends (`a.b.c` -> start of `a`).
    pub chain_start: usize,
    pub text: String,
}

const fn identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

const fn identifier_part(byte: u8) -> bool {
    identifier_start(byte) || byte.is_ascii_digit() || byte == b'$'
}

/// The index just past a quoted run that starts at `open` and doubles `quote`
/// to escape it. An unterminated run ends at the end of the text.
fn quoted_end(bytes: &[u8], open: usize, quote: u8) -> usize {
    let mut index = open + 1;
    while index < bytes.len() {
        if bytes[index] == quote {
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

fn unquote(raw: &str, quote: char) -> String {
    let inner = raw
        .strip_prefix(quote)
        .map_or(raw, |rest| rest.strip_suffix(quote).unwrap_or(rest));
    let doubled: String = [quote, quote].iter().collect();
    inner.replace(&doubled, &quote.to_string())
}

fn comment_end(bytes: &[u8], start: usize) -> Option<usize> {
    match (bytes.get(start), bytes.get(start + 1)) {
        (Some(b'-'), Some(b'-')) => Some(
            bytes[start..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |offset| start + offset),
        ),
        (Some(b'/'), Some(b'*')) => Some(
            bytes[start + 2..]
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(bytes.len(), |offset| start + offset + 4),
        ),
        _ => None,
    }
}

/// The string literals and identifiers of `sql`, in text order.
pub(super) fn tokens(sql: &str) -> Vec<Token> {
    let bytes = sql.as_bytes();
    let mut found: Vec<Token> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = comment_end(bytes, index) {
            index = end;
            continue;
        }
        let byte = bytes[index];
        let (kind, end, text) = if byte == b'\'' {
            let end = quoted_end(bytes, index, b'\'');
            (TokenKind::Literal, end, unquote(&sql[index..end], '\''))
        } else if byte == b'"' {
            let end = quoted_end(bytes, index, b'"');
            (TokenKind::Identifier, end, unquote(&sql[index..end], '"'))
        } else if identifier_start(byte) {
            let end = bytes[index..]
                .iter()
                .position(|byte| !identifier_part(*byte))
                .map_or(bytes.len(), |offset| index + offset);
            (
                TokenKind::Identifier,
                end,
                sql[index..end].to_ascii_lowercase(),
            )
        } else {
            index += 1;
            continue;
        };
        let chain_start = found
            .last()
            .filter(|previous| {
                kind == TokenKind::Identifier
                    && previous.kind == TokenKind::Identifier
                    && sql.get(previous.end..index) == Some(".")
            })
            .map_or(index, |previous| previous.chain_start);
        found.push(Token {
            kind,
            start: index,
            end,
            chain_start,
            text,
        });
        index = end;
    }
    found
}

/// `sql` with each `(start, end, replacement)` range replaced. Ranges must not
/// overlap; they are applied from the end of the text backwards.
pub(super) fn replace_ranges(sql: &str, ranges: &[(usize, usize, String)]) -> String {
    let mut ordered: Vec<&(usize, usize, String)> = ranges.iter().collect();
    ordered.sort_by_key(|range| std::cmp::Reverse(range.0));
    let mut rewritten = sql.to_owned();
    for (start, end, replacement) in ordered {
        rewritten.replace_range(*start..*end, replacement);
    }
    rewritten
}

/// `value` as a single-quoted SQL string literal.
pub(super) fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// `name` as a SQL identifier, quoted when folding would change it.
pub(super) fn quote_identifier(name: &str) -> String {
    let bare = name.as_bytes().first().is_some_and(|first| {
        (first.is_ascii_lowercase() || *first == b'_')
            && name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'$'
            })
    });
    if bare {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// Whether the literal `tokens[literal]` is cast to a type whose last name
/// part is `type_name`: `'x'::[schema.]t` or `CAST('x' AS [schema.]t)`.
pub(super) fn followed_by_cast_to(
    sql: &str,
    tokens: &[Token],
    literal: usize,
    type_name: &str,
) -> bool {
    let Some(token) = tokens.get(literal) else {
        return false;
    };
    let rest = sql[token.end..].trim_start();
    let offset = sql.len() - rest.len();
    let (type_start, function_form) = if let Some(after) = rest.strip_prefix("::") {
        (sql.len() - after.trim_start().len(), false)
    } else if rest
        .get(..2)
        .is_some_and(|keyword| keyword.eq_ignore_ascii_case("as"))
        && rest[2..].starts_with(char::is_whitespace)
    {
        let gap = rest[2..].len() - rest[2..].trim_start().len();
        (offset + 2 + gap, true)
    } else {
        return false;
    };
    // The cast target is the dotted identifier chain that starts right there.
    let Some(last) = tokens[literal + 1..]
        .iter()
        .skip_while(|next| next.start < type_start)
        .take_while(|next| next.kind == TokenKind::Identifier && next.chain_start == type_start)
        .last()
    else {
        return false;
    };
    // `'x' AS name` is a cast only inside `CAST(...)`; elsewhere it is an alias.
    let closes_cast = sql[last.end..].trim_start().starts_with([')', '[']);
    last.text == type_name && (!function_form || closes_cast)
}

#[cfg(test)]
#[path = "enum_view_text_tests.rs"]
mod tests;
