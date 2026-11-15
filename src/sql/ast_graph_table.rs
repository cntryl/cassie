//! The `GRAPH_TABLE` table source (#1256).
//!
//! The AST keeps `Expr` predicates and projections exactly as the ordinary
//! expression parser produced them. `Expr` has no `PartialEq`, so these types
//! derive only what their sibling statement types derive.

use serde::{Deserialize, Serialize};

use super::{Expr, IdentifierPath};

/// `GRAPH_TABLE ( graph MATCH [path_variable =] path [KEEP selector]
/// [WHERE predicate] COLUMNS (projection, ...) )`. The `AS alias` that may
/// follow is not part of this node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphTable {
    pub graph: IdentifierPath,
    /// The folded name bound to the whole path (`p = (a) ...`).
    pub path_variable: Option<String>,
    pub path: GraphPathPattern,
    /// The `KEEP` selector. `None` means `ALL WALK PATHS`.
    pub selector: Option<GraphSelector>,
    /// The graph-pattern `WHERE`, applied to selected complete paths.
    pub filter: Option<Expr>,
    pub columns: Vec<GraphColumn>,
}

impl GraphTable {
    /// The selector in force, with the documented default for an omitted `KEEP`.
    #[must_use]
    pub fn effective_selector(&self) -> GraphSelector {
        self.selector.unwrap_or_default()
    }
}

/// `vertex {edge [quantifier] vertex}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPathPattern {
    pub start: GraphVertexPattern,
    pub steps: Vec<GraphPathStep>,
}

/// One edge pattern and the vertex pattern it leads to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPathStep {
    pub edge: GraphEdgePattern,
    pub vertex: GraphVertexPattern,
}

/// `( [variable] [IS label {| label}] [WHERE predicate] )`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphVertexPattern {
    pub variable: Option<String>,
    /// Alternative labels; empty means no label restriction.
    pub labels: Vec<String>,
    pub filter: Option<Expr>,
}

/// `- [ ... ] ->`, `<- [ ... ] -` or `- [ ... ] -`, with an optional quantifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdgePattern {
    pub variable: Option<String>,
    /// Alternative labels; empty means no label restriction.
    pub labels: Vec<String>,
    pub filter: Option<Expr>,
    pub direction: GraphEdgeDirection,
    /// `None` means exactly one edge and a singleton binding.
    pub quantifier: Option<GraphQuantifier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphEdgeDirection {
    /// `-[..]->`
    Outgoing,
    /// `<-[..]-`
    Incoming,
    /// `-[..]-`
    Undirected,
}

/// Edge repetition. Bounds were checked at parse time (`min <= max`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphQuantifier {
    /// `{n}`
    Exact(u32),
    /// `{m,n}`, inclusive.
    Range { min: u32, max: u32 },
    /// `*`
    ZeroOrMore,
    /// `+`
    OneOrMore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GraphPathMode {
    #[default]
    Walk,
    Trail,
    Acyclic,
    Simple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GraphSelectorKind {
    /// `ALL [mode] PATHS`
    #[default]
    All,
    /// `ANY SHORTEST [mode] PATH`
    AnyShortest,
    /// `SHORTEST k [mode] PATHS`, with `k > 0`.
    Shortest(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GraphSelector {
    pub kind: GraphSelectorKind,
    pub mode: GraphPathMode,
}

/// `expression [AS column_alias]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphColumn {
    pub expr: Expr,
    pub alias: Option<String>,
}
