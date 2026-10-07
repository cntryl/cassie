//! Ordinary relation aliases retain delimiter spelling until namespace binding.

use super::{QuerySource, SqlError};
use crate::sql::ColumnIdentifierPath;

pub(super) fn parse(source: QuerySource, tail: &str) -> Result<QuerySource, SqlError> {
    let mut tail = super::super::lexical::trim_separators(tail);
    if tail.is_empty() {
        return Ok(source);
    }
    if let Some(end) = super::super::lexical::pattern_end(tail, 0, "as") {
        tail = super::super::lexical::trim_separators(&tail[end..]);
    }
    let mut end = 0;
    let mut quoted = false;
    let mut chars = tail.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if ch == '"' {
            if quoted && chars.peek().is_some_and(|(_, next)| *next == '"') {
                let _ = chars.next();
            } else {
                quoted = !quoted;
            }
        } else if !quoted && (ch.is_whitespace() || ch == '(') {
            break;
        }
        end = index + ch.len_utf8();
    }
    if quoted || end == 0 {
        return Err(SqlError::new("invalid relation alias".into()));
    }
    let alias = tail[..end].trim();
    let identifier = ColumnIdentifierPath::parse(alias).map_err(SqlError::new)?;
    if identifier.is_qualified()
        || (!alias.starts_with('"')
            && !alias
                .chars()
                .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$'))
    {
        return Err(SqlError::new("invalid relation alias".into()));
    }
    let rest = super::super::lexical::trim_separators(&tail[end..]);
    let column_aliases = if rest.is_empty() {
        Vec::new()
    } else {
        if !rest.starts_with('(') {
            return Err(SqlError::new("unsupported FROM syntax".into()));
        }
        let close = super::matching_closing_paren(rest)
            .ok_or_else(|| SqlError::new("invalid relation column aliases".into()))?;
        if !super::super::lexical::trim_separators(&rest[close + 1..]).is_empty() {
            return Err(SqlError::new("unsupported FROM syntax".into()));
        }
        super::split_csv(&rest[1..close])
            .into_iter()
            .map(super::super::identifiers::normalize_column_identifier)
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(QuerySource::Aliased {
        source: Box::new(source),
        alias: alias.to_string(),
        column_aliases,
    })
}
