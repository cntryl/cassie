//! `CREATE`, `DROP` and `ALTER PROPERTY GRAPH` over existing tables (#1255).

use serde::{Deserialize, Serialize};

use super::IdentifierPath;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyGraphStatement {
    pub action: PropertyGraphAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyGraphAction {
    Create(CreatePropertyGraph),
    Drop {
        name: IdentifierPath,
        if_exists: bool,
    },
    Rename {
        name: IdentifierPath,
        new_name: String,
    },
}

impl PropertyGraphStatement {
    /// The graph the statement names, as written.
    #[must_use]
    pub fn name(&self) -> &str {
        match &self.action {
            PropertyGraphAction::Create(graph) => graph.name.as_str(),
            PropertyGraphAction::Drop { name, .. } | PropertyGraphAction::Rename { name, .. } => {
                name.as_str()
            }
        }
    }
}

impl PropertyGraphAction {
    #[must_use]
    pub const fn command_tag(&self) -> &'static str {
        match self {
            Self::Create(_) => "CREATE PROPERTY GRAPH",
            Self::Drop { .. } => "DROP PROPERTY GRAPH",
            Self::Rename { .. } => "ALTER PROPERTY GRAPH",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePropertyGraph {
    pub name: IdentifierPath,
    pub vertices: Vec<GraphVertexMapping>,
    pub edges: Vec<GraphEdgeMapping>,
}

/// `table AS alias KEY (columns) LABEL ...`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphVertexMapping {
    pub table: IdentifierPath,
    pub alias: String,
    pub key: Vec<String>,
    pub labels: Vec<GraphLabel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdgeMapping {
    pub table: IdentifierPath,
    pub alias: String,
    pub key: Vec<String>,
    pub source: GraphEndpoint,
    pub destination: GraphEndpoint,
    pub labels: Vec<GraphLabel>,
}

/// `SOURCE|DESTINATION KEY (columns) REFERENCES vertex_alias (columns)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEndpoint {
    pub columns: Vec<String>,
    pub vertex_alias: String,
    pub references: Vec<String>,
}

/// `LABEL name PROPERTIES (column [AS property], ...)`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphLabel {
    pub name: String,
    pub properties: Vec<GraphProperty>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphProperty {
    pub column: String,
    pub name: String,
}
