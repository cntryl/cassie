//! Delimited (double-quoted) identifier handling for expression, DML-target
//! and alias positions.

use super::SqlError;

/// Parses a column reference written as one or more `.`-separated parts where
/// at least one part is a delimited identifier (`"name"`, `"tbl"."col"`,
/// `tbl."col"`). Delimited parts are unquoted and `""` collapses to `"`.
///
/// Returns `Ok(None)` when `raw` is not a pure identifier chain containing a
/// delimited part, so callers can fall through to other expression forms.
pub(super) fn parse_quoted_identifier_chain(raw: &str) -> Result<Option<String>, SqlError> {
    let raw = raw.trim();
    if !raw.contains('"') {
        return Ok(None);
    }

    let mut parts = Vec::new();
    let mut rest = raw;
    loop {
        let (part, after) = if let Some(body) = rest.strip_prefix('"') {
            let Some((part, consumed)) = take_delimited_body(body) else {
                return Ok(None);
            };
            if part.is_empty() {
                return Err(SqlError::new(
                    "zero-length delimited identifier".to_string(),
                ));
            }
            (part, &body[consumed..])
        } else {
            let len = rest
                .find(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '$'))
                .unwrap_or(rest.len());
            if len == 0 {
                return Ok(None);
            }
            (rest[..len].to_string(), &rest[len..])
        };
        parts.push(part);
        if after.is_empty() {
            return Ok(Some(parts.join(".")));
        }
        let Some(next) = after.strip_prefix('.') else {
            return Ok(None);
        };
        rest = next;
    }
}

/// Normalizes a single column name used as a DML target or alias: a
/// delimited identifier is unquoted, anything else is returned trimmed.
pub(super) fn normalize_identifier(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if !raw.starts_with('"') {
        return Ok(raw.to_string());
    }
    parse_quoted_identifier_chain(raw)?
        .ok_or_else(|| SqlError::new(format!("invalid delimited identifier '{raw}'")))
}

/// Reads a delimited identifier body (the text after the opening quote) and
/// returns the unescaped identifier plus the number of bytes consumed,
/// including the closing quote.
fn take_delimited_body(body: &str) -> Option<(String, usize)> {
    let mut value = String::new();
    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '"' {
            value.push(ch);
            continue;
        }
        if matches!(chars.peek(), Some((_, '"'))) {
            value.push('"');
            chars.next();
            continue;
        }
        return Some((value, idx + 1));
    }
    None
}
