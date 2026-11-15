//! The property graph section of the administrative catalog listing (#1255).

use super::{canonical_relation, QuerySchemaColumn, QuerySchemaItem};
use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::PropertyGraphMeta;

fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun} table")
    } else {
        format!("{count} {noun} tables")
    }
}

/// One entry per mapping, named by its alias and described by its kind and table.
fn elements(graph: &PropertyGraphMeta) -> Result<Vec<QuerySchemaColumn>, CassieError> {
    let vertices = graph
        .vertices
        .iter()
        .map(|vertex| ("vertex", &vertex.alias, &vertex.table));
    let edges = graph
        .edges
        .iter()
        .map(|edge| ("edge", &edge.alias, &edge.table));
    vertices
        .chain(edges)
        .map(|(kind, alias, table)| {
            let table = canonical_relation(table)?;
            Ok(QuerySchemaColumn {
                id: format!("element:{}:{kind}:{alias}", graph.name),
                name: alias.clone(),
                data_type: format!("{kind} on {}.{}", table.schema, table.name),
                primary_key: false,
            })
        })
        .collect()
}

/// The graphs of `database` that `session` may read every mapped table of.
pub(super) fn items(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    database: &str,
) -> Result<Vec<QuerySchemaItem>, CassieError> {
    crate::catalog::virtual_views::visible_property_graphs(&cassie.catalog, session, Some(database))
        .into_iter()
        .map(|graph| {
            let relation = canonical_relation(&graph.name)?;
            Ok(QuerySchemaItem {
                id: format!("property_graph:{}", graph.name),
                kind: "property_graph".to_owned(),
                label: graph.name.clone(),
                database: relation.database,
                schema: relation.schema,
                name: relation.name,
                metadata: Some(format!(
                    "{}, {}",
                    counted(graph.vertices.len(), "vertex"),
                    counted(graph.edges.len(), "edge")
                )),
                columns: elements(&graph)?,
            })
        })
        .collect()
}
