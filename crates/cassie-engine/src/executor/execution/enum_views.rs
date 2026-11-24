//! Views that depend on a named enum through their definition (#827).
//!
//! A view keeps its SQL text and is re-bound on every use. PostgreSQL tracks
//! view dependencies by object identity, so `DROP TYPE` is refused while a
//! view casts to the type and `ALTER TYPE ... RENAME` leaves every view
//! working. Here the definition is the dependency record: this module binds
//! it to find references to the enum and edits the text where a rename would
//! otherwise break it.

use super::enum_view_text::{
    followed_by_cast_to, quote_identifier, quote_literal, replace_ranges, tokens, Token, TokenKind,
};
use super::{Cassie, CassieSession};
use crate::app::CassieError;
use crate::catalog::enum_types::EnumDependent;
use crate::catalog::{EnumTypeMeta, ViewMeta};

/// A label no enum has, substituted for a literal to learn whether the binder
/// reads that literal as input of the enum.
const LABEL_PROBE: &str = "__cassie_enum_label_probe__";
/// A type name that does not exist, substituted for an identifier to learn
/// whether the parser reads that identifier as a type.
const TYPE_PROBE: &str = "cassie_enum_type_probe__";

/// A view whose definition text changes with the enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RewrittenView {
    pub name: String,
    pub query: String,
}

struct Binder<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
}

impl Binder<'_> {
    /// The bound statement of a view definition as JSON, the one form that
    /// exposes every embedded type without a visitor per AST node.
    fn bind(&self, query: &str) -> Result<serde_json::Value, CassieError> {
        let parsed = self
            .cassie
            .parse_statement_for_session(self.session, query)?;
        let context = self.cassie.binding_context_for_session(self.session);
        let bound = crate::sql::binder::bind_with_context(parsed, &self.cassie.catalog, &context)?;
        serde_json::to_value(&bound.statement.statement)
            .map_err(|error| CassieError::Parse(error.to_string()))
    }

    fn views(&self, database: &str) -> Vec<ViewMeta> {
        self.cassie
            .catalog
            .list_views()
            .into_iter()
            .filter(|view| {
                crate::catalog::relation_database_name(&view.name)
                    .is_none_or(|name| name.eq_ignore_ascii_case(database))
            })
            .collect()
    }
}

/// Whether a serialized statement embeds the descriptor of the enum `oid`.
fn mentions_enum(node: &serde_json::Value, oid: u32) -> bool {
    match node {
        serde_json::Value::Object(object) => {
            object.get("enum").is_some_and(|descriptor| {
                descriptor.get("members").is_some()
                    && descriptor.get("oid").and_then(serde_json::Value::as_u64)
                        == Some(u64::from(oid))
            }) || object.values().any(|child| mentions_enum(child, oid))
        }
        serde_json::Value::Array(items) => items.iter().any(|child| mentions_enum(child, oid)),
        _ => false,
    }
}

/// Views of the type's database whose definition references the enum without
/// exposing it as an output column (those are catalog dependents already).
pub(super) fn definition_dependents(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    metadata: &EnumTypeMeta,
    known: &[EnumDependent],
) -> Vec<EnumDependent> {
    let binder = Binder { cassie, session };
    let oid = metadata.type_oid();
    binder
        .views(&metadata.database)
        .into_iter()
        .filter(|view| !known.iter().any(|dependent| dependent.owner == view.name))
        .filter(|view| {
            binder
                .bind(&view.query)
                .is_ok_and(|statement| mentions_enum(&statement, oid))
        })
        .map(|view| EnumDependent {
            phrase: format!("view {}", crate::catalog::local_name(&view.name)),
            owner: view.name,
            array: false,
        })
        .collect()
}

fn fails_as(result: Result<serde_json::Value, CassieError>, state: &str, probe: &str) -> bool {
    result.is_err_and(|error| error.sql_state() == state && error.to_string().contains(probe))
}

/// Literals of `view` that are input of the enum and spell the label `from`.
fn label_ranges(
    binder: &Binder<'_>,
    view: &ViewMeta,
    type_name: &str,
    from: &str,
) -> Vec<(usize, usize)> {
    let found = tokens(&view.query);
    let probe = quote_literal(LABEL_PROBE);
    found
        .iter()
        .enumerate()
        .filter(|(_, token)| token.kind == TokenKind::Literal && token.text == from)
        .filter(|(index, token)| {
            followed_by_cast_to(&view.query, &found, *index, type_name)
                || fails_as(
                    binder.bind(&replace_ranges(
                        &view.query,
                        &[(token.start, token.end, probe.clone())],
                    )),
                    "22P02",
                    LABEL_PROBE,
                )
        })
        .map(|(_, token)| (token.start, token.end))
        .collect()
}

/// The view definitions that name the label `from` as input of the enum,
/// rewritten to `to`. A view that does not bind today is left untouched.
pub(super) fn rename_label(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    metadata: &EnumTypeMeta,
    from: &str,
    to: &str,
) -> Vec<RewrittenView> {
    let binder = Binder { cassie, session };
    let type_name = metadata.local_name().to_ascii_lowercase();
    let replacement = quote_literal(to);
    binder
        .views(&metadata.database)
        .into_iter()
        .filter(|view| view.query.contains(&quote_literal(from)))
        .filter(|view| binder.bind(&view.query).is_ok())
        .filter_map(|view| {
            let ranges = label_ranges(&binder, &view, &type_name, from)
                .into_iter()
                .map(|(start, end)| (start, end, replacement.clone()))
                .collect::<Vec<_>>();
            (!ranges.is_empty()).then(|| RewrittenView {
                query: replace_ranges(&view.query, &ranges),
                name: view.name,
            })
        })
        .collect()
}

/// Identifier tokens of `view` the parser reads as the enum's type name.
fn type_tokens<'t>(
    binder: &Binder<'_>,
    view: &ViewMeta,
    found: &'t [Token],
    local: &str,
) -> Vec<&'t Token> {
    found
        .iter()
        .filter(|token| token.kind == TokenKind::Identifier && token.text == local)
        .filter(|token| {
            fails_as(
                binder.bind(&replace_ranges(
                    &view.query,
                    &[(token.start, token.end, TYPE_PROBE.to_owned())],
                )),
                "42704",
                TYPE_PROBE,
            )
        })
        .collect()
}

/// The view definitions that name the enum as a type, rewritten for its new
/// name. `schema` is `Some` when the type moves: every reference then becomes
/// `schema.name`, because the old qualifier or search path no longer finds it.
pub(super) fn rename_type(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    metadata: &EnumTypeMeta,
    new_local: &str,
    schema: Option<&str>,
) -> Vec<RewrittenView> {
    let binder = Binder { cassie, session };
    let local = metadata.local_name().to_ascii_lowercase();
    let name = quote_identifier(new_local);
    binder
        .views(&metadata.database)
        .into_iter()
        .filter(|view| view.query.to_ascii_lowercase().contains(&local))
        .filter(|view| binder.bind(&view.query).is_ok())
        .filter_map(|view| {
            let found = tokens(&view.query);
            let ranges = type_tokens(&binder, &view, &found, &local)
                .into_iter()
                .map(|token| match schema {
                    Some(schema) => (
                        token.chain_start,
                        token.end,
                        format!("{}.{name}", quote_identifier(schema)),
                    ),
                    None => (token.start, token.end, name.clone()),
                })
                .collect::<Vec<_>>();
            (!ranges.is_empty()).then(|| RewrittenView {
                query: replace_ranges(&view.query, &ranges),
                name: view.name,
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "enum_views_tests.rs"]
mod tests;
