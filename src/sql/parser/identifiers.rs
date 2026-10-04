//! Delimited (double-quoted) identifier handling for expression, DML-target
//! and alias positions.

use super::SqlError;
use crate::sql::ast::IdentifierPath;
use crate::sql::ColumnIdentifierPath;

pub(super) fn parse_relation_path_prefix(raw: &str) -> Result<(IdentifierPath, &str), SqlError> {
    let raw = raw.trim_start();
    let mut in_quotes = false;
    // Comments are SQL separators even when they touch an identifier. Replacing
    // them with same-width spaces lets this scan find the boundary while the
    // returned tail still retains the original SQL bytes for clause parsing.
    let uncommented = super::lexical::without_comments(raw);
    let mut chars = uncommented.char_indices().peekable();
    while let Some((index, character)) = chars.next() {
        if character == '"' {
            if in_quotes && matches!(chars.peek(), Some((_, '"'))) {
                let _ = chars.next();
            } else {
                in_quotes = !in_quotes;
            }
        } else if character.is_whitespace() && !in_quotes {
            let path = IdentifierPath::parse(raw[..index].trim()).map_err(SqlError::new)?;
            return Ok((path, raw[index..].trim_start()));
        }
    }
    if in_quotes {
        return Err(SqlError::new(format!(
            "unterminated quoted identifier '{raw}'"
        )));
    }
    let path = IdentifierPath::parse(raw.trim()).map_err(SqlError::new)?;
    Ok((path, ""))
}

/// Parses a column reference written as one or more `.`-separated parts where
/// at least one part is a delimited identifier (`"name"`, `"tbl"."col"`,
/// `tbl."col"`). Delimited parts are unquoted and `""` collapses to `"`.
///
/// Returns `Ok(None)` when `raw` is not a pure identifier chain containing a
/// delimited part, so callers can fall through to other expression forms.
pub(super) fn parse_quoted_identifier_chain(
    raw: &str,
) -> Result<Option<ColumnIdentifierPath>, SqlError> {
    let raw = raw.trim();
    if !raw.contains('"') || raw.starts_with('\'') {
        return Ok(None);
    }
    match ColumnIdentifierPath::parse(raw) {
        Ok(path) => Ok(Some(path)),
        Err(error) if raw == "\"\"" => Err(SqlError::new(error)),
        Err(_) => Ok(None),
    }
}

/// Normalizes a single column name used as a DML target or alias: a
/// delimited identifier is unquoted, anything else is returned trimmed.
pub(super) fn normalize_identifier(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    if !raw.starts_with('"') {
        return Ok(raw.to_ascii_lowercase());
    }
    parse_quoted_identifier_chain(raw)?
        .map(|path| path.display_name())
        .ok_or_else(|| SqlError::new(format!("invalid delimited identifier '{raw}'")))
}

pub(super) fn normalize_column_identifier(raw: &str) -> Result<String, SqlError> {
    let raw = raw.trim();
    let path = ColumnIdentifierPath::parse(raw).map_err(SqlError::new)?;
    if path.is_qualified() {
        return Err(SqlError::new(format!("invalid column identifier '{raw}'")));
    }
    Ok(path.lookup_key())
}
