//! Keeps views working across `ALTER TYPE ... RENAME TO`, `SET SCHEMA` and
//! `RENAME ATTRIBUTE` of a composite type (#828).
//!
//! A view stores its definition as SQL text and binds types and attributes by
//! name, while PostgreSQL binds them by identity. The rewritten definitions are
//! published in the same transaction as the type, so a view never sees a type
//! name or attribute that no longer exists.

use std::collections::BTreeSet;

use super::domain_view_text::rewrite_cast_targets;
use super::sql_text_tokens::{chains, tokenize, Token};
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
///
/// `plain_qualifiers` are the folded names and aliases of the base tables the
/// view reads: `qualifier.from` there names a column of that table, never an
/// attribute of a composite value, so it neither needs a rewrite nor makes the
/// view ambiguous.
#[must_use]
pub(super) fn rename_field_selections(
    sql: &str,
    from: &str,
    to: &str,
    plain_qualifiers: &BTreeSet<String>,
) -> FieldRename {
    let tokens = tokenize(sql);
    let ambiguous = names_other_reference(sql, &tokens, from, plain_qualifiers);
    let mut rewritten = String::with_capacity(sql.len() + 16);
    let mut copied = 0;
    let mut changed = false;
    let mut depth = 0_usize;
    let mut past_select_list = false;
    for (index, token) in tokens.iter().enumerate() {
        if token.is_punct(sql, b'(') {
            depth += 1;
        } else if token.is_punct(sql, b')') {
            depth = depth.saturating_sub(1);
            let Some(name) = field_after_parenthesis(sql, &tokens, index, from) else {
                continue;
            };
            rewritten.push_str(&sql[copied..name.start]);
            rewritten.push_str(&quote_identifier(to));
            let ends_item = tokens
                .get(index + 3)
                .is_none_or(|next| next.is_punct(sql, b',') || next.is_keyword(sql, "from"));
            if depth == 0 && !past_select_list && ends_item {
                rewritten.push_str(" AS ");
                rewritten.push_str(&quote_identifier(from));
            }
            copied = name.end;
            changed = true;
        } else if depth == 0 && token.is_keyword(sql, "from") {
            past_select_list = true;
        }
    }
    let query = changed.then(|| {
        rewritten.push_str(&sql[copied..]);
        rewritten
    });
    FieldRename { query, ambiguous }
}

/// The attribute token of `(expr).attribute` when the parenthesis at `close`
/// ends a parenthesized expression that selects `from`.
fn field_after_parenthesis(sql: &str, tokens: &[Token], close: usize, from: &str) -> Option<Token> {
    if !tokens.get(close + 1)?.is_punct(sql, b'.') {
        return None;
    }
    let name = *tokens.get(close + 2)?;
    (name.identifier(sql).as_deref() == Some(from)).then_some(name)
}

/// Whether a dotted reference to `from` other than a parenthesized field
/// selection or a column of a base table of the view remains.
pub(super) fn names_other_reference(
    sql: &str,
    tokens: &[Token],
    from: &str,
    plain_qualifiers: &BTreeSet<String>,
) -> bool {
    chains(sql, tokens).iter().any(|chain| {
        let [first, .., last] = chain.parts.as_slice() else {
            return false;
        };
        tokens[*last].identifier(sql).as_deref() == Some(from)
            && tokens[*first]
                .identifier(sql)
                .is_none_or(|qualifier| !plain_qualifiers.contains(&qualifier))
    })
}

/// Whether `sql` selects `attribute` from a composite value: `(expr).attribute`
/// or a dotted reference whose qualifier is not a base table of the view.
pub(super) fn selects_attribute(
    sql: &str,
    attribute: &str,
    plain_qualifiers: &BTreeSet<String>,
) -> bool {
    let tokens = tokenize(sql);
    (0..tokens.len()).any(|index| {
        tokens[index].is_punct(sql, b')')
            && field_after_parenthesis(sql, &tokens, index, attribute).is_some()
    }) || names_other_reference(sql, &tokens, attribute, plain_qualifiers)
}

#[cfg(test)]
#[path = "composite_view_rewrite_tests.rs"]
mod tests;
