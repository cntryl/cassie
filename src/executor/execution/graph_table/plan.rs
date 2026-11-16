//! A bound `GRAPH_TABLE` resolved against its graph: pattern positions, selected mappings
//! and the properties each pattern element exposes to expressions.

use std::collections::HashSet;

use crate::catalog::{GraphLabelMeta, PropertyGraphMeta};
use crate::sql::ast::{
    Expr, GraphEdgeDirection, GraphQuantifier, GraphTable, GraphTableBinding, GraphVariableKind,
};
use crate::sql::ColumnIdentifierPath;

/// `(entry name, source column)` pairs, one per property of the element's variable.
pub(super) type Columns = Vec<(String, String)>;

/// Mappings a pattern position selects, indexed like the graph's mapping list.
pub(super) type Selection = Vec<Option<Columns>>;

pub(super) struct Filter<'a> {
    pub(super) expr: &'a Expr,
    /// References no pattern variable except its own element's.
    pub(super) local: bool,
}

pub(super) struct Position<'a> {
    pub(super) variable: Option<usize>,
    pub(super) selection: Selection,
    pub(super) filter: Option<Filter<'a>>,
    /// An earlier position naming the same variable: both bind one element.
    pub(super) repeat_of: Option<usize>,
}

pub(super) struct Step<'a> {
    pub(super) variable: Option<usize>,
    pub(super) group: bool,
    pub(super) direction: GraphEdgeDirection,
    pub(super) min: u32,
    pub(super) max: Option<u32>,
    pub(super) selection: Selection,
    pub(super) filter: Option<Filter<'a>>,
    /// An earlier unquantified step naming the same edge variable.
    pub(super) repeat_of: Option<usize>,
}

/// Where a scalar variable's element sits in a path.
#[derive(Clone, Copy)]
pub(super) enum Source {
    /// The vertex at this position.
    Vertex(usize),
    /// The single edge of this step.
    Edge(usize),
    /// A quantified edge variable: only its own inline filter can read it.
    Group,
}

pub(super) struct Variable {
    pub(super) name: String,
    pub(super) source: Source,
}

pub(super) struct Plan<'a> {
    pub(super) table: &'a GraphTable,
    pub(super) graph: PropertyGraphMeta,
    pub(super) variables: Vec<Variable>,
    /// `positions[0]` is the start; `positions[i + 1]` ends `steps[i]`.
    pub(super) positions: Vec<Position<'a>>,
    pub(super) steps: Vec<Step<'a>>,
    pub(super) deferred: bool,
    /// Adjacent quantified steps can match one path through different hop splits.
    pub(super) may_duplicate: bool,
}

fn internal(message: &str) -> crate::app::CassieError {
    crate::app::CassieError::Execution(message.to_owned())
}

fn requested(labels: &[String], label: &GraphLabelMeta) -> bool {
    labels.is_empty()
        || labels
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&label.name))
}

/// The mappings carrying any requested label (all when none is requested), each once, with
/// the columns of the variable's properties.
fn select(
    count: usize,
    labels_of: impl Fn(usize) -> Vec<GraphLabelMeta>,
    requested_labels: &[String],
    variable: Option<(&str, &[String])>,
) -> Selection {
    (0..count)
        .map(|mapping| {
            let carried = labels_of(mapping)
                .into_iter()
                .filter(|label| requested(requested_labels, label))
                .collect::<Vec<_>>();
            if carried.is_empty() {
                return None;
            }
            let mut columns = Columns::new();
            if let Some((name, properties)) = variable {
                for property in properties {
                    let found = carried.iter().find_map(|label| {
                        label
                            .properties
                            .iter()
                            .find(|candidate| candidate.name.eq_ignore_ascii_case(property))
                    });
                    if let Some(found) = found {
                        columns.push((format!("{name}.{property}"), found.column.clone()));
                    }
                }
            }
            Some(columns)
        })
        .collect()
}

fn referenced_variables(expr: &Expr, names: &HashSet<&str>, out: &mut HashSet<String>) {
    if let Expr::Column(name) = expr {
        if let Some(qualifier) = ColumnIdentifierPath::parse(name)
            .ok()
            .and_then(|path| path.namespace_qualifier())
        {
            if names.contains(qualifier.as_str()) {
                out.insert(qualifier);
            }
        }
    }
    let _ = expr.try_visit_children::<()>(|child| {
        referenced_variables(child, names, out);
        Ok(())
    });
}

fn filter_of<'a>(
    expr: Option<&'a Expr>,
    own: Option<&str>,
    names: &HashSet<&str>,
) -> Option<Filter<'a>> {
    let expr = expr?;
    let mut referenced = HashSet::new();
    referenced_variables(expr, names, &mut referenced);
    let local = referenced.iter().all(|name| Some(name.as_str()) == own);
    Some(Filter { expr, local })
}

fn bounds(quantifier: Option<GraphQuantifier>) -> (u32, Option<u32>) {
    match quantifier {
        None => (1, Some(1)),
        Some(GraphQuantifier::Exact(count)) => (count, Some(count)),
        Some(GraphQuantifier::Range { min, max }) => (min, Some(max)),
        Some(GraphQuantifier::ZeroOrMore) => (0, None),
        Some(GraphQuantifier::OneOrMore) => (1, None),
    }
}

impl<'a> Plan<'a> {
    /// # Errors
    ///
    /// An execution error when the table is unbound or its graph no longer exists.
    pub(super) fn build(
        table: &'a GraphTable,
        graph: PropertyGraphMeta,
    ) -> Result<Self, crate::app::CassieError> {
        let binding = table
            .binding
            .as_ref()
            .ok_or_else(|| internal("GRAPH_TABLE source was not bound"))?;
        let mut plan = Self {
            table,
            graph,
            variables: Vec::new(),
            positions: Vec::new(),
            steps: Vec::new(),
            deferred: false,
            may_duplicate: false,
        };
        plan.assemble(binding);
        Ok(plan)
    }

    fn property_names(binding: &GraphTableBinding, name: &str) -> Vec<String> {
        binding
            .variables
            .iter()
            .find(|variable| variable.name == name)
            .map(|variable| variable.properties.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_default()
    }

    fn variable_index(&self, name: &str) -> Option<usize> {
        self.variables
            .iter()
            .position(|variable| variable.name == name)
    }

    fn declare(&mut self, name: &str, source: Source) -> usize {
        if let Some(index) = self.variable_index(name) {
            return index;
        }
        self.variables.push(Variable {
            name: name.to_owned(),
            source,
        });
        self.variables.len() - 1
    }

    fn vertex_selection(&self, labels: &[String], name: Option<&str>, props: &[String]) -> Selection {
        select(
            self.graph.vertices.len(),
            |mapping| self.graph.vertices[mapping].labels.clone(),
            labels,
            name.map(|name| (name, props)),
        )
    }

    fn edge_selection(&self, labels: &[String], name: Option<&str>, props: &[String]) -> Selection {
        select(
            self.graph.edges.len(),
            |mapping| self.graph.edges[mapping].labels.clone(),
            labels,
            name.map(|name| (name, props)),
        )
    }

    fn assemble(&mut self, binding: &GraphTableBinding) {
        let table = self.table;
        let names = binding
            .variables
            .iter()
            .filter(|variable| variable.kind != GraphVariableKind::Path)
            .map(|variable| variable.name.as_str())
            .collect::<HashSet<_>>();
        let pattern = &table.path;
        self.push_position(binding, &names, &pattern.start, 0);
        for (index, step) in pattern.steps.iter().enumerate() {
            let (min, max) = bounds(step.edge.quantifier);
            let group = step.edge.quantifier.is_some();
            let name = step.edge.variable.as_deref();
            let variable = name.map(|name| {
                let source = if group {
                    Source::Group
                } else {
                    Source::Edge(index)
                };
                self.declare(name, source)
            });
            let properties = name
                .map(|name| Self::property_names(binding, name))
                .unwrap_or_default();
            let repeat_of = variable.and_then(|variable| {
                self.steps
                    .iter()
                    .position(|earlier| earlier.variable == Some(variable))
            });
            let filter = filter_of(step.edge.filter.as_ref(), name, &names);
            self.deferred |= filter.as_ref().is_some_and(|filter| !filter.local);
            self.steps.push(Step {
                variable,
                group,
                direction: step.edge.direction,
                min,
                max,
                selection: self.edge_selection(&step.edge.labels, name, &properties),
                filter,
                repeat_of,
            });
            self.push_position(binding, &names, &step.vertex, index + 1);
        }
        self.may_duplicate = self
            .steps
            .iter()
            .filter(|step| step.max != Some(step.min))
            .count()
            > 1;
    }

    fn push_position(
        &mut self,
        binding: &GraphTableBinding,
        names: &HashSet<&str>,
        vertex: &'a crate::sql::ast::GraphVertexPattern,
        index: usize,
    ) {
        let name = vertex.variable.as_deref();
        let variable = name.map(|name| self.declare(name, Source::Vertex(index)));
        let properties = name
            .map(|name| Self::property_names(binding, name))
            .unwrap_or_default();
        let repeat_of = variable.and_then(|variable| {
            self.positions
                .iter()
                .position(|earlier| earlier.variable == Some(variable))
        });
        let filter = filter_of(vertex.filter.as_ref(), name, names);
        self.deferred |= filter.as_ref().is_some_and(|filter| !filter.local);
        self.positions.push(Position {
            variable,
            selection: self.vertex_selection(&vertex.labels, name, &properties),
            filter,
            repeat_of,
        });
    }

    /// Vertex mappings read: every selected by a position, and the endpoint mappings of
    /// every selected edge mapping (an edge needs both endpoints to exist).
    pub(super) fn vertex_mappings_read(&self) -> Vec<usize> {
        let mut wanted = vec![false; self.graph.vertices.len()];
        for position in &self.positions {
            for (index, selected) in position.selection.iter().enumerate() {
                wanted[index] |= selected.is_some();
            }
        }
        for mapping in self.edge_mappings_read() {
            let edge = &self.graph.edges[mapping];
            for alias in [&edge.source.vertex_alias, &edge.destination.vertex_alias] {
                if let Some(index) = self
                    .graph
                    .vertices
                    .iter()
                    .position(|vertex| &vertex.alias == alias)
                {
                    wanted[index] = true;
                }
            }
        }
        (0..wanted.len()).filter(|index| wanted[*index]).collect()
    }

    pub(super) fn edge_mappings_read(&self) -> Vec<usize> {
        (0..self.graph.edges.len())
            .filter(|mapping| {
                self.steps
                    .iter()
                    .any(|step| step.selection[*mapping].is_some())
            })
            .collect()
    }
}
