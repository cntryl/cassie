//! The exact `CREATE PROPERTY GRAPH` statement of a stored definition (#1354).
//!
//! The text names the graph and every mapped table by schema and local name only, never by
//! database, so a restored database image that holds the same tables under the same schemas
//! can run it unchanged. Identities (database, graph and mapping IDs) are deliberately not
//! part of it: running the statement in a clone allocates fresh ones.

use super::object_identity::quote_identifier;
use super::{
    local_name, relation_schema_name, GraphEdgeMeta, GraphEndpointMeta, GraphKeyColumn,
    GraphLabelMeta, GraphVertexMeta, PropertyGraphMeta,
};

fn qualified(canonical: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(&relation_schema_name(canonical)),
        quote_identifier(&local_name(canonical))
    )
}

fn columns<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let list = names
        .into_iter()
        .map(quote_identifier)
        .collect::<Vec<_>>()
        .join(", ");
    format!("({list})")
}

fn key(key: &[GraphKeyColumn]) -> String {
    columns(key.iter().map(|column| column.name.as_str()))
}

fn labels(labels: &[GraphLabelMeta]) -> String {
    labels
        .iter()
        .map(|label| {
            let properties = label
                .properties
                .iter()
                .map(|property| {
                    if property.name == property.column {
                        quote_identifier(&property.column)
                    } else {
                        format!(
                            "{} AS {}",
                            quote_identifier(&property.column),
                            quote_identifier(&property.name)
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "LABEL {} PROPERTIES ({properties})",
                quote_identifier(&label.name)
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn vertex(vertex: &GraphVertexMeta) -> String {
    format!(
        "{} AS {} KEY {} {}",
        qualified(&vertex.table),
        quote_identifier(&vertex.alias),
        key(&vertex.key),
        labels(&vertex.labels)
    )
}

fn endpoint(keyword: &str, endpoint: &GraphEndpointMeta, graph: &PropertyGraphMeta) -> String {
    let references = graph
        .vertices
        .iter()
        .find(|vertex| vertex.alias == endpoint.vertex_alias)
        .map_or_else(String::new, |vertex| key(&vertex.key));
    format!(
        "{keyword} KEY {} REFERENCES {} {references}",
        columns(endpoint.columns.iter().map(String::as_str)),
        quote_identifier(&endpoint.vertex_alias)
    )
}

fn edge(edge: &GraphEdgeMeta, graph: &PropertyGraphMeta) -> String {
    format!(
        "{} AS {} KEY {} {} {} {}",
        qualified(&edge.table),
        quote_identifier(&edge.alias),
        key(&edge.key),
        endpoint("SOURCE", &edge.source, graph),
        endpoint("DESTINATION", &edge.destination, graph),
        labels(&edge.labels)
    )
}

/// The `CREATE PROPERTY GRAPH` statement that recreates `graph`: vertex tables first, then
/// edge tables, each in the definition's own order.
#[must_use]
pub fn render_definition(graph: &PropertyGraphMeta) -> String {
    let vertices = graph
        .vertices
        .iter()
        .map(vertex)
        .collect::<Vec<_>>()
        .join(", ");
    let edges = graph
        .edges
        .iter()
        .map(|candidate| edge(candidate, graph))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "CREATE PROPERTY GRAPH {} VERTEX TABLES ({vertices}) EDGE TABLES ({edges})",
        qualified(&graph.name)
    )
}
