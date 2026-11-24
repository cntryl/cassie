//! Views that depend on an attribute of a composite type, found from the
//! parsed view definitions and followed transitively through views.
//!
//! A view reads a composite value when it reads a table column of the type,
//! exposes a column of the type itself, names the type in a cast, or reads
//! another view that does. Whether such a view selects the attribute is then
//! decided from the tokens of its definition: `(expr).attribute` selects it,
//! while `table.attribute` names a column of a base table and does not.

use std::collections::BTreeSet;

use super::composite_view_rewrite::selects_attribute;
use super::sql_text_tokens::{tokenize, Kind};
use super::view_dependents::{parsed_select, views_reading, with_transitive_dependents};
use super::Cassie;
use crate::catalog::composite_types::data_type_depends_on;
use crate::catalog::{CompositeTypeMeta, ViewMeta};
use crate::sql::ast::{QuerySource, SelectStatement};

/// The folded last part of a written relation name.
fn last_part(written: &str) -> Option<String> {
    let tokens = tokenize(written);
    tokens
        .iter()
        .rev()
        .find(|token| matches!(token.kind, Kind::Word | Kind::Quoted))
        .and_then(|token| token.identifier(written))
}

fn collect_plain(cassie: &Cassie, source: &QuerySource, found: &mut BTreeSet<String>) {
    match source {
        QuerySource::Collection(path) => {
            let written = path.to_string();
            if cassie.catalog.get_collection(&written).is_some() {
                found.extend(last_part(&written));
            }
        }
        QuerySource::Aliased { source, alias, .. } => {
            if matches!(**source, QuerySource::Collection(_)) {
                let mut inner = BTreeSet::new();
                collect_plain(cassie, source, &mut inner);
                if !inner.is_empty() {
                    found.insert(alias.to_ascii_lowercase());
                }
            }
        }
        QuerySource::TableSample { source, .. } => collect_plain(cassie, source, found),
        QuerySource::Join { left, right, .. } => {
            collect_plain(cassie, left, found);
            collect_plain(cassie, right, found);
        }
        _ => {}
    }
}

fn collect_select(cassie: &Cassie, select: &SelectStatement, found: &mut BTreeSet<String>) {
    collect_plain(cassie, &select.source, found);
    if let Some(set) = &select.set {
        collect_select(cassie, &set.right, found);
    }
}

/// The folded names and aliases under which `view` reads a base table: a dotted
/// reference through one of them names a column of that table.
pub(super) fn plain_qualifiers(cassie: &Cassie, view: &ViewMeta) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    if let Some(select) = parsed_select(cassie, view) {
        collect_select(cassie, &select, &mut found);
    }
    found
}

/// Whether the cast targets of `view` name the composite type.
fn names_type(view: &ViewMeta, type_name: &str) -> bool {
    tokenize(&view.query)
        .iter()
        .any(|token| token.identifier(&view.query).as_deref() == Some(type_name))
}

/// The views that select `attribute` from a value of the composite type.
pub(super) fn views_selecting_attribute(
    cassie: &Cassie,
    metadata: &CompositeTypeMeta,
    attribute: &str,
) -> Vec<ViewMeta> {
    let oid = metadata.type_oid();
    let type_name = metadata.local_name().to_ascii_lowercase();
    let mut seeds = Vec::new();
    for collection in cassie.catalog.list_collections_canonical() {
        let carries = cassie
            .catalog
            .get_schema(&collection.name)
            .is_some_and(|schema| {
                schema
                    .fields
                    .iter()
                    .any(|field| data_type_depends_on(&field.data_type, oid))
            });
        if carries {
            seeds.extend(views_reading(cassie, &collection.name));
        }
    }
    for view in cassie.catalog.list_views() {
        let exposes = view
            .schema
            .fields
            .iter()
            .any(|field| data_type_depends_on(&field.data_type, oid));
        if exposes || names_type(&view, &type_name) {
            seeds.push(view);
        }
    }
    let mut views = Vec::new();
    for view in with_transitive_dependents(cassie, seeds) {
        let qualifiers = plain_qualifiers(cassie, &view);
        if selects_attribute(&view.query, attribute, &qualifiers)
            && views.iter().all(|known: &ViewMeta| known.name != view.name)
        {
            views.push(view);
        }
    }
    views
}
