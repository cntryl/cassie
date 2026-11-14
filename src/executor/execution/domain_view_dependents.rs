//! Views whose definition casts to a domain (#828).
//!
//! A view stores its definition as SQL text, so its dependency on a domain is
//! found by parsing that text and watching which type names it resolves.
//! `DROP DOMAIN` reports or cascades to those views, and `ALTER DOMAIN ...
//! RENAME TO` / `SET SCHEMA` renames the references so the views keep working.

use std::cell::Cell;
use std::rc::Rc;

use super::domain_view_text::rewrite_cast_targets;
use super::{Cassie, QueryError};
use crate::catalog::object_identity::quote_identifier;
use crate::catalog::{parse_name, relation_database_name, DomainTypeMeta, ParsedName, ViewMeta};
use crate::sql::ast::{ParsedStatement, QueryStatement};
use crate::sql::{
    parse_statement, with_domain_scope, with_enum_scope, with_type_scope, DomainResolver,
};

/// Whether the type name `raw` can denote `domain`: the same unqualified name,
/// or the same name qualified by the domain's schema (and database).
fn names_domain(raw: &str, domain: &DomainTypeMeta) -> bool {
    let local = domain.local_name();
    match parse_name(raw) {
        Ok(ParsedName::Unqualified(name)) => name.eq_ignore_ascii_case(&local),
        Ok(ParsedName::SchemaQualified { schema, name }) => {
            name.eq_ignore_ascii_case(&local) && schema.eq_ignore_ascii_case(&domain.schema_name())
        }
        Ok(ParsedName::DatabaseQualified {
            database,
            schema,
            name,
        }) => {
            name.eq_ignore_ascii_case(&local)
                && schema.eq_ignore_ascii_case(&domain.schema_name())
                && database.eq_ignore_ascii_case(&domain.database)
        }
        Err(_) => false,
    }
}

/// Parses a stored view definition with every user type name in scope. When
/// `watched` is given, a type name that denotes it resolves to it and sets
/// `named`.
fn parse_view(
    cassie: &Cassie,
    query: &str,
    watched: Option<(&DomainTypeMeta, &Rc<Cell<bool>>)>,
) -> Option<ParsedStatement> {
    let inner = cassie.domain_type_resolver(None);
    let domains: DomainResolver = match watched {
        Some((domain, named)) => {
            let domain = domain.clone();
            let named = Rc::clone(named);
            let recording: DomainResolver = Rc::new(move |raw: &str| {
                if names_domain(raw, &domain) {
                    named.set(true);
                    return Some(domain.descriptor());
                }
                inner(raw)
            });
            recording
        }
        None => inner,
    };
    let types = cassie.composite_type_resolver(None);
    let enums = cassie.enum_type_resolver(None);
    with_enum_scope(enums, || {
        with_domain_scope(domains, || {
            with_type_scope(types, || parse_statement(query))
        })
    })
    .ok()
}

fn in_database(view: &ViewMeta, database: &str) -> bool {
    relation_database_name(&view.name).is_none_or(|name| name.eq_ignore_ascii_case(database))
}

/// Views of the domain's database whose definition casts to `domain`.
pub(super) fn direct_dependents(cassie: &Cassie, domain: &DomainTypeMeta) -> Vec<ViewMeta> {
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| in_database(view, &domain.database))
        .filter(|view| {
            let named = Rc::new(Cell::new(false));
            let _ = parse_view(cassie, &view.query, Some((domain, &named)));
            named.get()
        })
        .collect()
}

/// Whether the definition of `view` selects from one of `targets`.
fn selects_from(cassie: &Cassie, view: &ViewMeta, targets: &[ViewMeta]) -> bool {
    let Some(ParsedStatement {
        statement: QueryStatement::Select(select),
        ..
    }) = parse_view(cassie, &view.query, None)
    else {
        return false;
    };
    crate::sql::binder::select_referenced_relations(&select)
        .iter()
        .filter_map(|relation| cassie.catalog.get_view(relation))
        .any(|referenced| targets.iter().any(|target| target.name == referenced.name))
}

/// The views a cascade removes with `domain`: those that cast to it and,
/// transitively, the views that select from them.
fn cascade_dependents(cassie: &Cassie, domain: &DomainTypeMeta) -> Vec<ViewMeta> {
    let mut dependents = direct_dependents(cassie, domain);
    if dependents.is_empty() {
        return dependents;
    }
    loop {
        let next = cassie
            .catalog
            .list_views()
            .into_iter()
            .filter(|view| in_database(view, &domain.database))
            .filter(|view| dependents.iter().all(|known| known.name != view.name))
            .find(|view| selects_from(cassie, view, &dependents));
        match next {
            Some(view) => dependents.push(view),
            None => return dependents,
        }
    }
}

/// `DROP ... CASCADE`: removes every view that depends on `domain`.
pub(super) fn drop_dependent_views(
    cassie: &Cassie,
    domain: &DomainTypeMeta,
) -> Result<(), QueryError> {
    for view in cascade_dependents(cassie, domain) {
        cassie
            .midge
            .defer_drop_view(&view.name, cassie.runtime.schema_epoch())
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_view(&view.name);
    }
    Ok(())
}

/// The dependent views of `previous` with every cast to it renamed to
/// `updated`, schema-qualified so the reference does not depend on the search
/// path.
pub(super) fn renamed_views(
    cassie: &Cassie,
    previous: &DomainTypeMeta,
    updated: &DomainTypeMeta,
) -> Vec<ViewMeta> {
    let replacement = format!(
        "{}.{}",
        quote_identifier(&updated.schema_name()),
        quote_identifier(&updated.local_name())
    );
    direct_dependents(cassie, previous)
        .into_iter()
        .filter_map(|mut view| {
            let query = rewrite_cast_targets(&view.query, &|name: &str| {
                names_domain(name, previous).then(|| replacement.clone())
            })?;
            view.query = query;
            Some(view)
        })
        .collect()
}

/// Persists and publishes rewritten view definitions.
pub(super) fn publish_views(cassie: &Cassie, views: Vec<ViewMeta>) -> Result<(), QueryError> {
    for view in views {
        cassie.midge.put_view(&view).map_err(QueryError::from)?;
        cassie.catalog.register_view(view);
    }
    Ok(())
}
