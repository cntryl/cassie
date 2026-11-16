//! The `GRAPH_TABLE` table source (#1256).
//!
//! The AST keeps `Expr` predicates and projections exactly as the ordinary
//! expression parser produced them. `Expr` has no `PartialEq`, so these types
//! derive only what their sibling statement types derive.

use serde::{Deserialize, Serialize};

use super::{Expr, IdentifierPath};
use crate::types::DataType;

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
    /// What binding resolved; `None` until the binder has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<GraphTableBinding>,
}

impl GraphTable {
    /// Every expression of the table in a fixed order: inline predicates of the
    /// pattern from left to right, the pattern `WHERE`, then the projections.
    pub fn expressions(&self) -> impl Iterator<Item = &Expr> {
        let steps = self
            .path
            .steps
            .iter()
            .flat_map(|step| [step.edge.filter.as_ref(), step.vertex.filter.as_ref()]);
        std::iter::once(self.path.start.filter.as_ref())
            .chain(steps)
            .flatten()
            .chain(self.filter.as_ref())
            .chain(self.columns.iter().map(|column| &column.expr))
    }

    /// Names the pattern declares: element variables and the path variable.
    pub fn variable_names(&self) -> impl Iterator<Item = &str> {
        let steps = self.path.steps.iter().flat_map(|step| {
            [
                step.edge.variable.as_deref(),
                step.vertex.variable.as_deref(),
            ]
        });
        std::iter::once(self.path.start.variable.as_deref())
            .chain(steps)
            .chain(std::iter::once(self.path_variable.as_deref()))
            .flatten()
    }

    /// The number of positions [`GraphTable::expression_at`] addresses: one filter slot per
    /// pattern element, the pattern `WHERE` slot and one slot per projection.
    #[must_use]
    pub fn expression_slots(&self) -> usize {
        1 + 2 * self.path.steps.len() + 1 + self.columns.len()
    }

    /// The expression at `slot`, in the order of [`GraphTable::expressions`]; `None` for an
    /// absent filter.
    #[must_use]
    pub fn expression_at(&self, slot: usize) -> Option<&Expr> {
        let elements = 1 + 2 * self.path.steps.len();
        if slot == 0 {
            return self.path.start.filter.as_ref();
        }
        if slot < elements {
            let step = &self.path.steps[(slot - 1) / 2];
            return if (slot - 1).is_multiple_of(2) {
                step.edge.filter.as_ref()
            } else {
                step.vertex.filter.as_ref()
            };
        }
        if slot == elements {
            return self.filter.as_ref();
        }
        self.columns
            .get(slot - elements - 1)
            .map(|column| &column.expr)
    }

    /// Mutable counterpart of [`GraphTable::expressions`], in the same order.
    pub fn expressions_mut(&mut self) -> impl Iterator<Item = &mut Expr> {
        let steps = self
            .path
            .steps
            .iter_mut()
            .flat_map(|step| [step.edge.filter.as_mut(), step.vertex.filter.as_mut()]);
        std::iter::once(self.path.start.filter.as_mut())
            .chain(steps)
            .flatten()
            .chain(self.filter.as_mut())
            .chain(self.columns.iter_mut().map(|column| &mut column.expr))
    }

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

impl GraphColumn {
    /// The output column name: the alias, or the property or variable a bare
    /// reference names. `None` for any other unaliased expression.
    #[must_use]
    pub fn output_name(&self) -> Option<String> {
        match (&self.alias, &self.expr) {
            (Some(alias), _) => Some(alias.clone()),
            (None, Expr::Column(name)) => {
                Some(name.rsplit('.').next().unwrap_or(name.as_str()).to_owned())
            }
            (None, _) => None,
        }
    }
}

/// The result of binding a [`GraphTable`] against a mapped property graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphTableBinding {
    /// Canonical `database.schema.graph` name.
    pub graph: String,
    /// Canonical names of the mapped tables the pattern can read.
    pub tables: Vec<String>,
    pub variables: Vec<GraphVariableBinding>,
    /// The types of the `COLUMNS` projections, in order.
    pub output: Vec<DataType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphVariableKind {
    Vertex,
    Edge,
    Path,
}

/// A pattern variable and the mappings and properties it can bind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphVariableBinding {
    pub name: String,
    pub kind: GraphVariableKind,
    /// `true` for an edge variable under a quantifier.
    pub group: bool,
    /// Mapping identities the variable can bind, without duplicates.
    pub mappings: Vec<String>,
    /// Properties that bind to one type across every selected mapping.
    pub properties: Vec<(String, DataType)>,
}
