//! Property graphs mapped over existing tables (#1255).
//!
//! A definition stores no rows. It names vertex and edge tables, their key
//! columns, endpoints, labels and exposed properties. Graph, mapping and
//! database identities are durable UUIDs: a rename keeps them and a dropped and
//! recreated object receives new ones.

use serde::{Deserialize, Serialize};

use super::metadata::Catalog;
use crate::types::DataType;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyGraphMeta {
    /// Canonical `database.schema.graph` name.
    pub name: String,
    #[serde(default)]
    pub object_id: u64,
    /// Durable identity of this graph, as a lowercase canonical UUID.
    pub graph_id: String,
    /// Durable identity of the owning database.
    pub database_id: String,
    pub vertices: Vec<GraphVertexMeta>,
    pub edges: Vec<GraphEdgeMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphVertexMeta {
    pub mapping_id: String,
    pub alias: String,
    /// Canonical name of the mapped table.
    pub table: String,
    pub key: Vec<GraphKeyColumn>,
    pub labels: Vec<GraphLabelMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEdgeMeta {
    pub mapping_id: String,
    pub alias: String,
    pub table: String,
    pub key: Vec<GraphKeyColumn>,
    pub source: GraphEndpointMeta,
    pub destination: GraphEndpointMeta,
    pub labels: Vec<GraphLabelMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphKeyColumn {
    pub name: String,
    pub data_type: DataType,
}

/// The edge columns that hold one endpoint's vertex key, in key order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphEndpointMeta {
    pub columns: Vec<String>,
    pub vertex_alias: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphLabelMeta {
    pub name: String,
    pub properties: Vec<GraphPropertyMeta>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphPropertyMeta {
    pub name: String,
    pub column: String,
    pub data_type: DataType,
}

impl PropertyGraphMeta {
    /// Canonical names of every table this graph maps, without duplicates.
    #[must_use]
    pub fn mapped_tables(&self) -> Vec<&str> {
        let mut tables = self
            .vertices
            .iter()
            .map(|vertex| vertex.table.as_str())
            .chain(self.edges.iter().map(|edge| edge.table.as_str()))
            .collect::<Vec<_>>();
        tables.sort_unstable();
        tables.dedup();
        tables
    }

    /// Whether `column` of `table` is a key or endpoint column of a mapping.
    #[must_use]
    pub fn references_key_column(&self, table: &str, column: &str) -> bool {
        let same = |candidate: &str| candidate.eq_ignore_ascii_case(column);
        let vertex_key = self
            .vertices
            .iter()
            .filter(|vertex| vertex.table.eq_ignore_ascii_case(table))
            .any(|vertex| vertex.key.iter().any(|key| same(&key.name)));
        let edge_column = self
            .edges
            .iter()
            .filter(|edge| edge.table.eq_ignore_ascii_case(table))
            .any(|edge| {
                edge.key.iter().any(|key| same(&key.name))
                    || edge.source.columns.iter().any(|name| same(name))
                    || edge.destination.columns.iter().any(|name| same(name))
            });
        vertex_key || edge_column
    }
}

impl Catalog {
    pub fn register_property_graph(&self, metadata: PropertyGraphMeta) {
        self.property_graphs
            .write()
            .insert(metadata.name.to_ascii_lowercase(), metadata);
        self.bump_version();
    }

    pub fn unregister_property_graph(&self, name: &str) {
        self.property_graphs
            .write()
            .remove(&name.to_ascii_lowercase());
        self.bump_version();
    }

    /// The graph stored under exactly this canonical name.
    #[must_use]
    pub fn get_property_graph(&self, canonical: &str) -> Option<PropertyGraphMeta> {
        self.property_graphs
            .read()
            .get(&canonical.to_ascii_lowercase())
            .cloned()
    }

    #[must_use]
    pub fn list_property_graphs(&self) -> Vec<PropertyGraphMeta> {
        let mut out = self
            .property_graphs
            .read()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        out.sort_by_key(|graph| graph.name.to_ascii_lowercase());
        out
    }

    /// Graphs that map `table`, matched by canonical name.
    #[must_use]
    pub fn property_graphs_mapping_table(&self, table: &str) -> Vec<PropertyGraphMeta> {
        self.list_property_graphs()
            .into_iter()
            .filter(|graph| {
                graph
                    .mapped_tables()
                    .iter()
                    .any(|mapped| mapped.eq_ignore_ascii_case(table))
            })
            .collect()
    }
}
