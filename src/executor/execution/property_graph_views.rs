//! Views that read a property graph through `GRAPH_TABLE` (#1256).
//!
//! A view stores its definition as SQL text, so its dependency on a graph is found by
//! parsing that text. Dropping the graph while such a view exists would leave the view
//! unusable, so it is refused with `2BP01`, as for any other object a view reads.

use super::domain_view_dependents::parse_view;
use super::{Cassie, QueryError};
use crate::app::DiagnosticError;
use crate::catalog::PropertyGraphMeta;
use crate::sql::ast::{ParsedStatement, QueryStatement};
use crate::sql::binder::BindingContext;

const DEPENDENT_OBJECTS: &str = "2BP01";

/// Names of the views whose definition reads `graph`.
fn dependent_views(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &PropertyGraphMeta,
) -> Vec<String> {
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| {
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
                    crate::sql::binder::resolve_existing_name(name, context, |candidate| {
                        cassie.catalog.get_property_graph(candidate).is_some()
                    })
                    .is_ok_and(|resolved| resolved.eq_ignore_ascii_case(&graph.name))
                })
        })
        .map(|view| view.name)
        .collect()
}

/// Refuses to drop a graph that a view reads.
pub(super) fn reject_drop(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &PropertyGraphMeta,
) -> Result<(), QueryError> {
    let views = dependent_views(cassie, context, graph);
    if views.is_empty() {
        return Ok(());
    }
    let detail = views
        .iter()
        .map(|name| format!("view {name}"))
        .collect::<Vec<_>>()
        .join(", ");
    Err(QueryError::Cassie(
        DiagnosticError::new(
            DEPENDENT_OBJECTS,
            format!(
                "cannot drop property graph {} because other objects depend on it",
                crate::catalog::local_name(&graph.name)
            ),
        )
        .with_detail(format!("dependent objects: {detail}"))
        .into(),
    ))
}
