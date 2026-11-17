//! Views that read a property graph through `GRAPH_TABLE` (#1256).
//!
//! A view stores its definition as SQL text, so its dependency on a graph is found by
//! parsing that text. Dropping the graph while such a view exists would leave the view
//! unusable, so it is refused with `2BP01`, as for any other object a view reads.

use super::domain_view_dependents::{cascade_from, parse_view, view_context, view_database};
use super::{Cassie, QueryError};
use crate::app::DiagnosticError;
use crate::catalog::{PropertyGraphMeta, ViewMeta};
use crate::sql::ast::{ParsedStatement, QueryStatement};
use crate::sql::binder::BindingContext;

const DEPENDENT_OBJECTS: &str = "2BP01";

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
            let view_context = view_context(cassie, view);
            let Some(ParsedStatement {
                statement: QueryStatement::Select(select),
                ..
            }) = parse_view(cassie, &view.query, None)
            else {
                return false;
            };
            crate::sql::binder::select_referenced_graphs(&select)
                .iter()
                .any(|name| {
                    crate::sql::binder::resolve_existing_name(name, &view_context, |candidate| {
                        cassie.catalog.get_property_graph(candidate).is_some()
                    })
                    .is_ok_and(|resolved| crate::catalog::name_matches(&graph.name, &resolved))
                })
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

/// Remove direct graph views and their transitive same-database view dependents.
pub(super) fn drop_dependents(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &PropertyGraphMeta,
) -> Result<(), QueryError> {
    for view in cascade_from(
        cassie,
        &context.database,
        dependent_views(cassie, context, graph),
    ) {
        cassie
            .midge
            .defer_drop_view(&view.name, cassie.runtime.schema_epoch())
            .map_err(|error| QueryError::General(error.to_string()))?;
        cassie.catalog.unregister_view(&view.name);
    }
    Ok(())
}
