//! The elements a complete path binds to the pattern variables, as expression rows.

use super::eval::{element_entries, Entries};
use super::plan::{Plan, Source};
use super::store::Store;

/// A complete path match: interleaved vertices, edges and direction values (1 forward,
/// 2 reverse), and the hop count of each pattern step.
#[derive(Clone)]
pub(super) struct Candidate {
    pub(super) vertices: Vec<usize>,
    pub(super) edges: Vec<usize>,
    pub(super) directions: Vec<u8>,
    pub(super) step_hops: Vec<usize>,
}

impl Candidate {
    /// Bytes the candidate retains.
    pub(super) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + (self.vertices.capacity() + self.edges.capacity() + self.step_hops.capacity()) * 4
            + self.directions.capacity()
    }

    pub(super) fn hop_count(&self) -> usize {
        self.edges.len()
    }

    /// Index into the path's vertices of the vertex at pattern `position`, or of the first
    /// edge of pattern step `position`.
    pub(super) fn offset(&self, position: usize) -> usize {
        self.step_hops[..position].iter().copied().sum()
    }
}

/// `variable.property` entries of every scalar (non-group) variable of the path.
pub(super) fn scalar_entries(plan: &Plan<'_>, store: &Store, candidate: &Candidate) -> Entries {
    let mut entries = Entries::new();
    for variable in &plan.variables {
        match variable.source {
            Source::Vertex(position) => {
                let (mapping, row) =
                    store.vertex_row(candidate.vertices[candidate.offset(position)]);
                if let Some(columns) = &plan.positions[position].selection[mapping] {
                    entries.extend(element_entries(row, columns));
                }
            }
            Source::Edge(step) => {
                let (mapping, row) = store.edge_row(candidate.edges[candidate.offset(step)]);
                if let Some(columns) = &plan.steps[step].selection[mapping] {
                    entries.extend(element_entries(row, columns));
                }
            }
            Source::Group => {}
        }
    }
    entries
}

/// The entries of one edge of a quantified step under its group variable's name.
pub(super) fn group_entries(plan: &Plan<'_>, store: &Store, step: usize, edge: usize) -> Entries {
    let (mapping, row) = store.edge_row(edge);
    plan.steps[step].selection[mapping]
        .as_ref()
        .map_or_default(|columns| element_entries(row, columns))
}
