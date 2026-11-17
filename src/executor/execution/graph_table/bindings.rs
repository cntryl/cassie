//! The elements a complete path binds to the pattern variables, as expression rows.

use super::eval::{element_entries, push_entry, Entries};
use super::plan::{Plan, Source};
use super::store::Store;
use super::{QueryError, SourceExecutionEnv};
use crate::runtime::accounted::{Accounted, AccountedVec};

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
            + (self.vertices.capacity() + self.edges.capacity() + self.step_hops.capacity())
                * std::mem::size_of::<usize>()
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
pub(super) fn scalar_entries(
    env: &SourceExecutionEnv<'_>,
    plan: &Plan<'_>,
    store: &Store,
    candidate: &Candidate,
) -> Result<Accounted<Entries>, QueryError> {
    let mut entries = AccountedVec::try_new(env.controls)?;
    for variable in &plan.variables {
        match variable.source {
            Source::Vertex(position) => {
                let (mapping, row) =
                    store.vertex_row(candidate.vertices[candidate.offset(position)]);
                if let Some(columns) = &plan.positions[position].selection[mapping] {
                    for (entry, column) in columns {
                        env.controls.check_interruption()?;
                        push_entry(&mut entries, entry, row.get(column))?;
                    }
                }
            }
            Source::Edge(step) => {
                let (mapping, row) = store.edge_row(candidate.edges[candidate.offset(step)]);
                if let Some(columns) = &plan.steps[step].selection[mapping] {
                    for (entry, column) in columns {
                        env.controls.check_interruption()?;
                        push_entry(&mut entries, entry, row.get(column))?;
                    }
                }
            }
            Source::Group => {}
        }
    }
    let (entries, memory) = entries.into_parts();
    Ok(Accounted::from_admitted(entries, memory))
}

/// The entries of one edge of a quantified step under its group variable's name.
pub(super) fn group_entries(
    env: &SourceExecutionEnv<'_>,
    plan: &Plan<'_>,
    store: &Store,
    step: usize,
    edge: usize,
) -> Result<Accounted<Entries>, QueryError> {
    let (mapping, row) = store.edge_row(edge);
    element_entries(
        env,
        row,
        plan.steps[step].selection[mapping]
            .as_deref()
            .unwrap_or(&[]),
    )
}

#[cfg(test)]
mod tests {
    use super::Candidate;
    use std::mem::size_of;

    #[test]
    fn should_account_for_the_native_width_of_every_candidate_index() {
        // Arrange
        let candidate = Candidate {
            vertices: vec![0, 1, 2],
            edges: vec![0, 1],
            directions: vec![1, 1],
            step_hops: vec![1, 1],
        };
        let retained_indices = candidate.vertices.capacity()
            + candidate.edges.capacity()
            + candidate.step_hops.capacity();
        let expected = size_of::<Candidate>()
            + retained_indices * size_of::<usize>()
            + candidate.directions.capacity() * size_of::<u8>();

        // Act
        let accounted = candidate.bytes();

        // Assert
        assert_eq!(accounted, expected);
    }
}
