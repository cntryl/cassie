//! Keeps views working across `ALTER TYPE ... RENAME TO`, `SET SCHEMA` and
//! `RENAME ATTRIBUTE` of a composite type (#828).
//!
//! A view stores its definition as SQL text and binds types and attributes by
//! name, while PostgreSQL binds them by identity. The rewritten definitions are
//! published in the same transaction as the type, so a view never sees a type
//! name or attribute that no longer exists.

use super::domain_view_text::rewrite_cast_targets;
use crate::catalog::object_identity::quote_identifier;
use crate::catalog::{parse_name, relation_database_name, CompositeTypeMeta, ParsedName, ViewMeta};

/// Whether the type name `raw` can denote `metadata`: the same unqualified
/// name, or the same name qualified by its schema (and database).
fn names_type(raw: &str, metadata: &CompositeTypeMeta) -> bool {
    let local = metadata.local_name();
    match parse_name(raw) {
        Ok(ParsedName::Unqualified(name)) => name == local,
        Ok(ParsedName::SchemaQualified { schema, name }) => {
            name == local && schema == metadata.schema_name()
        }
        Ok(ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        }) => name == local && schema == metadata.schema_name() && database == metadata.database,
        Err(_) => false,
    }
}

fn in_database(view: &ViewMeta, database: &str) -> bool {
    relation_database_name(&view.name).is_none_or(|name| name == database)
}

/// The views of the type's database whose casts to `current` now name `next`.
#[must_use]
pub(super) fn renamed_type_views(
    views: Vec<ViewMeta>,
    current: &CompositeTypeMeta,
    next: &CompositeTypeMeta,
) -> Vec<ViewMeta> {
    let replacement = format!(
        "{}.{}",
        quote_identifier(&next.schema_name()),
        quote_identifier(&next.local_name())
    );
    views
        .into_iter()
        .filter(|view| in_database(view, &current.database))
        .filter_map(|mut view| {
            view.query = rewrite_cast_targets(&view.query, &|name: &str| {
                names_type(name, current).then(|| replacement.clone())
            })?;
            Some(view)
        })
        .collect()
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 0x80
}

fn skip_whitespace(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

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

/// The identifier starting at `start`: its end and its folded name (bare names
/// fold to lower case, quoted names keep their exact spelling).
fn identifier_at(sql: &str, start: usize) -> Option<(usize, String)> {
    let bytes = sql.as_bytes();
    if bytes.get(start) == Some(&b'"') {
        let end = quoted_end(bytes, start, b'"');
        let inner = sql.get(start + 1..end.checked_sub(1)?)?;
        return Some((end, inner.replace("\"\"", "\"")));
    }
    let mut end = start;
    while end < bytes.len() && is_identifier_byte(bytes[end]) {
        end += 1;
    }
    (end > start && !bytes[start].is_ascii_digit())
        .then(|| (end, sql[start..end].to_ascii_lowercase()))
}

fn keyword_at(bytes: &[u8], index: usize, keyword: &[u8]) -> bool {
    bytes
        .get(index..index + keyword.len())
        .is_some_and(|word| word.eq_ignore_ascii_case(keyword))
        && (index == 0 || !is_identifier_byte(bytes[index - 1]))
        && !bytes
            .get(index + keyword.len())
            .is_some_and(|byte| is_identifier_byte(*byte))
}

/// The outcome of renaming the field selections of one view definition.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct FieldRename {
    /// The rewritten definition, when any `(expr).from` was renamed.
    pub query: Option<String>,
    /// Whether a `.from` reference that is not a parenthesized field
    /// selection remains, so the rewrite cannot prove the view keeps working.
    pub ambiguous: bool,
}

/// Renames each `(expr).from` field selection in `sql` to `(expr).to`. An
/// unaliased select-list item keeps its output column name with `AS from`, as
/// PostgreSQL's view keeps the column names it was created with.
#[must_use]
pub(super) fn rename_field_selections(sql: &str, from: &str, to: &str) -> FieldRename {
    let bytes = sql.as_bytes();
    let mut rewritten = String::with_capacity(sql.len() + 16);
    let mut copied = 0;
    let mut changed = false;
    let mut ambiguous = false;
    let mut depth = 0_usize;
    let mut past_select_list = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' => index = quoted_end(bytes, index, b'\''),
            b'(' => {
                depth += 1;
                index += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                let dot = skip_whitespace(bytes, index + 1);
                index += 1;
                if bytes.get(dot) != Some(&b'.') {
                    continue;
                }
                let start = skip_whitespace(bytes, dot + 1);
                let Some((end, name)) = identifier_at(sql, start) else {
                    continue;
                };
                if name != from {
                    continue;
                }
                rewritten.push_str(&sql[copied..start]);
                rewritten.push_str(&quote_identifier(to));
                let next = skip_whitespace(bytes, end);
                let ends_item =
                    next >= bytes.len() || bytes[next] == b',' || keyword_at(bytes, next, b"from");
                if depth == 0 && !past_select_list && ends_item {
                    rewritten.push_str(" AS ");
                    rewritten.push_str(&quote_identifier(from));
                }
                copied = end;
                changed = true;
                index = end;
            }
            b'.' => {
                let start = skip_whitespace(bytes, index + 1);
                if identifier_at(sql, start).is_some_and(|(_, name)| name == from) {
                    ambiguous = true;
                }
                index += 1;
            }
            byte => {
                if depth == 0 && keyword_at(bytes, index, b"from") {
                    past_select_list = true;
                }
                index += if byte == b'"' {
                    quoted_end(bytes, index, b'"') - index
                } else {
                    1
                };
            }
        }
    }
    let query = changed.then(|| {
        rewritten.push_str(&sql[copied..]);
        rewritten
    });
    FieldRename { query, ambiguous }
}

#[cfg(test)]
#[path = "composite_view_rewrite_tests.rs"]
mod tests;
