//! Views that read a property graph through `GRAPH_TABLE` (#1256).
//!
//! A view stores its definition as SQL text, so its dependency on a graph is found by
//! parsing that text. Dropping the graph while such a view exists would leave the view
//! unusable, so it is refused with `2BP01`, as for any other object a view reads.

use super::domain_view_dependents::{parse_view, view_database};
use super::{Cassie, QueryError};
use crate::app::DiagnosticError;
use crate::catalog::{PropertyGraphMeta, ViewMeta};
use crate::sql::ast::{ParsedStatement, QueryStatement, SelectStatement};
use crate::sql::binder::BindingContext;

const DEPENDENT_OBJECTS: &str = "2BP01";

fn referenced_names(
    cassie: &Cassie,
    view: &ViewMeta,
    collect: fn(&SelectStatement) -> Vec<String>,
) -> Vec<String> {
    let Some(ParsedStatement {
        statement: QueryStatement::Select(select),
        ..
    }) = parse_view(cassie, &view.query, None)
    else {
        return Vec::new();
    };
    collect(&select)
}

/// The same-database views whose stored definition reads `graph`.
fn dependent_views(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &PropertyGraphMeta,
) -> Vec<ViewMeta> {
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| view_database(cassie, view) == context.database)
        .filter(|view| {
            referenced_names(cassie, view, crate::sql::binder::select_referenced_graphs)
                .iter()
                .any(|name| crate::catalog::name_matches(&graph.name, name))
        })
        .collect()
}

/// Refuses to drop a graph that a view reads.
pub(super) fn reject_drop(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &PropertyGraphMeta,
) -> Result<(), QueryError> {
    reject_change(cassie, context, graph, "drop")
}

pub(super) fn reject_rename(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &PropertyGraphMeta,
) -> Result<(), QueryError> {
    reject_change(cassie, context, graph, "rename")
}

fn reject_change(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &PropertyGraphMeta,
    action: &str,
) -> Result<(), QueryError> {
    let views = dependent_views(cassie, context, graph);
    if views.is_empty() {
        return Ok(());
    }
    let detail = views
        .iter()
        .map(|view| format!("view {}", view.name))
        .collect::<Vec<_>>()
        .join(", ");
    Err(QueryError::Cassie(
        DiagnosticError::new(
            DEPENDENT_OBJECTS,
            format!(
                "cannot {action} property graph {} because other objects depend on it",
                crate::catalog::local_name(&graph.name)
            ),
        )
        .with_detail(format!("dependent objects: {detail}"))
        .into(),
    ))
}

fn same_database(cassie: &Cassie, name: &str, database: &str) -> bool {
    crate::catalog::relation_database_name(name)
        .as_deref()
        .unwrap_or(&cassie.default_database)
        == database
}

fn potential_targets<'a>(
    cassie: &Cassie,
    database: &str,
    reference: &str,
    names: &'a [String],
) -> Vec<&'a str> {
    names
        .iter()
        .filter(|name| same_database(cassie, name, database))
        .filter(|name| crate::catalog::name_matches(name, reference))
        .map(String::as_str)
        .collect()
}

fn require_unique(view: &ViewMeta, reference: &str, count: usize) -> Result<(), QueryError> {
    if count > 1 {
        return Err(QueryError::Cassie(
            DiagnosticError::new(
                "0A000",
                format!("cannot cascade graph views through ambiguous reference {reference}"),
            )
            .with_detail(format!("view {} has multiple potential targets", view.name))
            .with_hint("Qualify the stored view reference before dropping the graph schema.")
            .into(),
        ));
    }
    Ok(())
}

/// Compute the entire graph-specific closure without guessing a view's execution
/// search path. Table and materialized-relation shadows count alongside views.
fn cascade_dependents(
    cassie: &Cassie,
    targets: &[PropertyGraphMeta],
) -> Result<Vec<ViewMeta>, QueryError> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let views = cassie.catalog.list_views();
    let graphs = cassie
        .catalog
        .list_property_graphs()
        .into_iter()
        .map(|graph| graph.name)
        .collect::<Vec<_>>();
    let mut relations = cassie
        .catalog
        .list_collections_canonical()
        .into_iter()
        .map(|collection| collection.name)
        .chain(views.iter().map(|view| view.name.clone()))
        .chain(
            cassie
                .catalog
                .list_projection_metadata()
                .into_iter()
                .filter(|projection| {
                    projection.kind == crate::catalog::ProjectionKind::Materialized
                })
                .map(|projection| projection.collection),
        )
        .collect::<Vec<_>>();
    relations.sort();
    relations.dedup();
    let mut dependents = Vec::new();
    for view in &views {
        let database = view_database(cassie, view);
        for reference in
            referenced_names(cassie, view, crate::sql::binder::select_referenced_graphs)
        {
            let candidates = potential_targets(cassie, &database, &reference, &graphs);
            if candidates
                .iter()
                .any(|name| targets.iter().any(|graph| graph.name == *name))
            {
                require_unique(view, &reference, candidates.len())?;
                dependents.push(view.clone());
                break;
            }
        }
    }
    loop {
        let mut next = None;
        for view in &views {
            if dependents.iter().any(|known| known.name == view.name) {
                continue;
            }
            let database = view_database(cassie, view);
            for reference in referenced_names(
                cassie,
                view,
                crate::sql::binder::select_referenced_relations,
            ) {
                let candidates = potential_targets(cassie, &database, &reference, &relations);
                if candidates
                    .iter()
                    .any(|name| dependents.iter().any(|known| known.name == *name))
                {
                    require_unique(view, &reference, candidates.len())?;
                    next = Some(view.clone());
                    break;
                }
            }
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(view) => dependents.push(view),
            None => return Ok(dependents),
        }
    }
}

/// Preflight every schema-owned graph before any view or graph is removed.
pub(super) fn drop_dependents(
    cassie: &Cassie,
    graphs: &[PropertyGraphMeta],
) -> Result<(), QueryError> {
    for view in cascade_dependents(cassie, graphs)? {
        cassie
            .midge
            .defer_drop_view(&view.name, cassie.runtime.schema_epoch())
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_view(&view.name);
    }
    Ok(())
}
