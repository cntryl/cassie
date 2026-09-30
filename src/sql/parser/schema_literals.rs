//! Constant literals written inside DDL: `CHECK` comparison operands, column
//! `DEFAULT` values and role passwords.
//!
//! A single-quoted literal decodes `''` to one quote, exactly as the
//! expression parser does, so a constant means the same thing wherever it is
//! written.

use super::{strip_parentheses, SqlError, Value};

/// Parses one SQL constant: `NULL`, `TRUE`, `FALSE`, a single-quoted string
/// or a number, optionally parenthesized and followed by a `::type` cast (as
/// `pg_dump` writes `'draft'::varchar`). The cast is dropped because the
/// value is coerced to the column type when the DDL is bound. Anything else,
/// such as a column reference or an expression, is an error rather than
/// being reinterpreted as text.
pub(super) fn parse_constant_literal(raw: &str) -> Result<Value, SqlError> {
    let raw = strip_literal_cast(raw.trim());
    if let Some(inner) = strip_parentheses(raw) {
        return parse_constant_literal(inner);
    }
    if raw.is_empty() {
        return Err(SqlError::new("invalid literal".to_string()));
    }
    if raw.eq_ignore_ascii_case("null") {
        return Ok(Value::Null);
    }
    if raw.eq_ignore_ascii_case("true") {
        return Ok(Value::Bool(true));
    }
    if raw.eq_ignore_ascii_case("false") {
        return Ok(Value::Bool(false));
    }
    if let Some(text) = single_quoted_literal(raw) {
        return Ok(Value::String(text));
    }
    if let Ok(value) = raw.parse::<i64>() {
        return Ok(value.into());
    }
    if let Ok(value) = raw.parse::<f64>() {
        return Ok(value.into());
    }
    Err(SqlError::unsupported(format!(
        "'{raw}' is not a constant: only NULL, TRUE, FALSE, a quoted string or a number is supported here"
    )))
}

/// Parses a role password. A constant decodes as
/// [`parse_constant_literal`]; other text is kept verbatim.
pub(in crate::sql::parser) fn parse_constraint_literal(raw: &str) -> Result<Value, SqlError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(SqlError::new("invalid literal".to_string()));
    }
    if let Ok(value) = parse_constant_literal(raw) {
        return Ok(value);
    }
    let text = raw
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(raw);
    Ok(Value::String(text.to_string()))
}

/// Decodes a single-quoted SQL string literal, turning each doubled `''`
/// into one quote. Returns `None` when `raw` is not exactly one literal, for
/// example `'a' || 'b'`.
fn single_quoted_literal(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut decoded = String::with_capacity(inner.len());
    let mut characters = inner.chars();
    while let Some(character) = characters.next() {
        if character == '\'' && characters.next() != Some('\'') {
            return None;
        }
        decoded.push(character);
    }
    Some(decoded)
}

/// Drops one trailing `::type` cast outside any quoted text, for example
/// `'draft'::varchar` or `'{}'::jsonb`. Text whose cast target is not a
/// plain type name is returned unchanged.
fn strip_literal_cast(raw: &str) -> &str {
    let mut in_single = false;
    let mut cast_at = None;
    for (index, character) in raw.char_indices() {
        match character {
            '\'' => in_single = !in_single,
            ':' if !in_single && raw[index + 1..].starts_with(':') => {
                cast_at = Some(index);
                break;
            }
            _ => {}
        }
    }
    let Some(cast_at) = cast_at else {
        return raw;
    };
    let type_name = raw[cast_at + 2..].trim();
    let is_type_name = !type_name.is_empty()
        && type_name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '(' | ')' | '[' | ']')
        });
    if is_type_name {
        raw[..cast_at].trim_end()
    } else {
        raw
    }
}
