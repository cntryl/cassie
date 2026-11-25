//! The `GRAPH_TABLE` table source (#1256).
//!
//! The AST keeps `Expr` predicates and projections exactly as the ordinary
//! expression parser produced them. `Expr` has no `PartialEq`, so these types
//! derive only what their sibling statement types derive.

use serde::{Deserialize, Serialize};

use super::{Expr, IdentifierPath};
use cassie_core::types::DataType;

/// Path patterns one `GRAPH_TABLE` may join on shared vertex variables (#1353).
pub const MAX_GRAPH_PATTERNS: usize = 2;

/// `GRAPH_TABLE ( graph MATCH [path_variable =] path [KEEP selector]
/// [WHERE predicate] COLUMNS (projection, ...) )`. The `AS alias` that may
/// follow is not part of this node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct GraphTable<A = ()> {
    pub graph: IdentifierPath,
    /// The folded name bound to the whole path (`p = (a) ...`).
    pub path_variable: Option<String>,
    pub path: GraphPathPattern<A>,
    /// The comma-separated patterns after the first (#1353). Each joins the earlier ones on
    /// the vertex variables they share.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_paths: Vec<GraphPathPattern<A>>,
    /// The `KEEP` selector. `None` means `ALL WALK PATHS`.
    pub selector: Option<GraphSelector>,
    /// The graph-pattern `WHERE`, applied to selected complete paths.
    pub filter: Option<Expr<A>>,
    pub columns: Vec<GraphColumn<A>>,
    /// What binding resolved; `None` until the binder has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<GraphTableBinding>,
}

impl<A> GraphTable<A> {
    /// Every path pattern of the table, in source order: the first and then the
    /// comma-separated patterns that join it on shared vertex variables.
    pub fn patterns(&self) -> impl Iterator<Item = &GraphPathPattern<A>> {
        std::iter::once(&self.path).chain(self.extra_paths.iter())
    }

    /// Every expression of the table in a fixed order: inline predicates of each
    /// pattern from left to right, the pattern `WHERE`, then the projections.
    pub fn expressions(&self) -> impl Iterator<Item = &Expr<A>> {
        self.patterns()
            .flat_map(GraphPathPattern::filters)
            .flatten()
            .chain(self.filter.as_ref())
            .chain(self.columns.iter().map(|column| &column.expr))
    }

    /// Names the pattern declares: element variables and the path variable.
    pub fn variable_names(&self) -> impl Iterator<Item = &str> {
        self.patterns()
            .flat_map(GraphPathPattern::variable_names)
            .chain(self.path_variable.as_deref())
    }

    /// The number of positions [`GraphTable::expression_at`] addresses: one filter slot per
    /// pattern element, the pattern `WHERE` slot and one slot per projection.
    #[must_use]
    pub fn expression_slots(&self) -> usize {
        self.patterns()
            .map(GraphPathPattern::filter_slots)
            .sum::<usize>()
            + 1
            + self.columns.len()
    }

    /// The expression at `slot`, in the order of [`GraphTable::expressions`]; `None` for an
    /// absent filter.
    #[must_use]
    pub fn expression_at(&self, slot: usize) -> Option<&Expr<A>> {
        let mut slot = slot;
        for pattern in self.patterns() {
            let count = pattern.filter_slots();
            if slot < count {
                return pattern.filter_at(slot);
            }
            slot -= count;
        }
        if slot == 0 {
            return self.filter.as_ref();
        }
        self.columns.get(slot - 1).map(|column| &column.expr)
    }

    /// Mutable counterpart of [`GraphTable::expressions`], in the same order.
    pub fn expressions_mut(&mut self) -> impl Iterator<Item = &mut Expr<A>> {
        std::iter::once(&mut self.path)
            .chain(self.extra_paths.iter_mut())
            .flat_map(GraphPathPattern::filters_mut)
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
#[serde(bound(serialize = "", deserialize = ""))]
pub struct GraphPathPattern<A = ()> {
    pub start: GraphVertexPattern<A>,
    pub steps: Vec<GraphPathStep<A>>,
}

impl<A> GraphPathPattern<A> {
    /// The inline predicate slots in order: the start vertex, then each step's edge and
    /// vertex.
    pub fn filters(&self) -> impl Iterator<Item = Option<&Expr<A>>> {
        let steps = self
            .steps
            .iter()
            .flat_map(|step| [step.edge.filter.as_ref(), step.vertex.filter.as_ref()]);
        std::iter::once(self.start.filter.as_ref()).chain(steps)
    }

    /// Mutable counterpart of [`GraphPathPattern::filters`].
    pub fn filters_mut(&mut self) -> impl Iterator<Item = Option<&mut Expr<A>>> {
        let steps = self
            .steps
            .iter_mut()
            .flat_map(|step| [step.edge.filter.as_mut(), step.vertex.filter.as_mut()]);
        std::iter::once(self.start.filter.as_mut()).chain(steps)
    }

    /// One filter slot per pattern element.
    #[must_use]
    pub fn filter_slots(&self) -> usize {
        1 + 2 * self.steps.len()
    }

    /// The inline predicate in `slot` of [`GraphPathPattern::filters`].
    #[must_use]
    pub fn filter_at(&self, slot: usize) -> Option<&Expr<A>> {
        if slot == 0 {
            return self.start.filter.as_ref();
        }
        let step = self.steps.get((slot - 1) / 2)?;
        if (slot - 1).is_multiple_of(2) {
            step.edge.filter.as_ref()
        } else {
            step.vertex.filter.as_ref()
        }
    }

    /// Element variables the pattern names, vertices and edges.
    pub fn variable_names(&self) -> impl Iterator<Item = &str> {
        let steps = self.steps.iter().flat_map(|step| {
            [
                step.edge.variable.as_deref(),
                step.vertex.variable.as_deref(),
            ]
        });
        std::iter::once(self.start.variable.as_deref())
            .chain(steps)
            .flatten()
    }

    /// Vertex variables the pattern names.
    pub fn vertex_variable_names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.start.variable.as_deref())
            .chain(
                self.steps
                    .iter()
                    .map(|step| step.vertex.variable.as_deref()),
            )
            .flatten()
    }

    /// Edge variables the pattern names.
    pub fn edge_variable_names(&self) -> impl Iterator<Item = &str> {
        self.steps
            .iter()
            .filter_map(|step| step.edge.variable.as_deref())
    }
}

/// One edge pattern and the vertex pattern it leads to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct GraphPathStep<A = ()> {
    pub edge: GraphEdgePattern<A>,
    pub vertex: GraphVertexPattern<A>,
}

/// `( [variable] [IS label {| label}] [WHERE predicate] )`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct GraphVertexPattern<A = ()> {
    pub variable: Option<String>,
    /// Alternative labels; empty means no label restriction.
    pub labels: Vec<String>,
    pub filter: Option<Expr<A>>,
}

/// `- [ ... ] ->`, `<- [ ... ] -` or `- [ ... ] -`, with an optional quantifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct GraphEdgePattern<A = ()> {
    pub variable: Option<String>,
    /// Alternative labels; empty means no label restriction.
    pub labels: Vec<String>,
    pub filter: Option<Expr<A>>,
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
#[serde(bound(serialize = "", deserialize = ""))]
pub struct GraphColumn<A = ()> {
    pub expr: Expr<A>,
    pub alias: Option<String>,
}

impl<A> GraphColumn<A> {
    /// The output column name: the alias, or the property or variable a bare
    /// reference names. `None` for any other unaliased expression.
    #[must_use]
    pub fn output_name(&self) -> Option<String> {
        match (&self.alias, &self.expr) {
            (Some(alias), _) => Some(alias.clone()),
            (None, Expr::Column(name)) => {
                crate::column_identifier::ColumnIdentifierPath::parse(name)
                    .ok()
                    .map(|path| path.implicit_output_label())
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
