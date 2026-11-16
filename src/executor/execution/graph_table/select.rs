//! Selectors and the documented total order of tied paths.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::bindings::Candidate;
use super::store::Store;
use super::{QueryError, SourceExecutionEnv};
use crate::sql::ast::GraphSelectorKind;

/// Compares paths lexicographically over `V0, E0, direction0, V1, E1, direction1, V2, ...`.
/// Vertex and edge ranks follow the order of durable references (kind, mapping ID, key
/// tuple), forward (1) sorts before reverse (2) and a path that is a prefix of another sorts
/// first.
pub(super) fn order(store: &Store, left: &Candidate, right: &Candidate) -> Ordering {
    let shared = left.vertices.len().min(right.vertices.len());
    for index in 0..shared {
        let by_vertex =
            store.vertex_rank[left.vertices[index]].cmp(&store.vertex_rank[right.vertices[index]]);
        if by_vertex != Ordering::Equal {
            return by_vertex;
        }
        if index >= left.edges.len() || index >= right.edges.len() {
            break;
        }
        let by_edge = store.edge_rank[left.edges[index]].cmp(&store.edge_rank[right.edges[index]]);
        if by_edge != Ordering::Equal {
            return by_edge;
        }
        let by_direction = left.directions[index].cmp(&right.directions[index]);
        if by_direction != Ordering::Equal {
            return by_direction;
        }
    }
    left.hop_count().cmp(&right.hop_count())
}

/// Applies the selector. `SHORTEST k` and `ANY SHORTEST` rank by hop count, then by the
/// total path order, separately for each pair of start and end vertices. The result is in
/// path order.
///
/// # Errors
///
/// Cancellation or deadline during ranking.
pub(super) fn apply(
    env: &SourceExecutionEnv<'_>,
    store: &Store,
    kind: GraphSelectorKind,
    mut candidates: Vec<Candidate>,
) -> Result<Vec<Candidate>, QueryError> {
    env.controls.check_interruption()?;
    let limit = match kind {
        GraphSelectorKind::All => {
            candidates.sort_by(|left, right| order(store, left, right));
            return Ok(candidates);
        }
        GraphSelectorKind::AnyShortest => 1,
        GraphSelectorKind::Shortest(count) => count as usize,
    };
    let mut pairs = HashMap::<(usize, usize), Vec<usize>>::new();
    for (index, path) in candidates.iter().enumerate() {
        env.controls.check_interruption()?;
        let end = *path.vertices.last().expect("path has a vertex");
        pairs
            .entry((path.vertices[0], end))
            .or_default()
            .push(index);
    }
    let mut chosen = Vec::new();
    for (_, mut group) in pairs {
        env.controls.check_interruption()?;
        group.sort_by(|left, right| {
            let (left, right) = (&candidates[*left], &candidates[*right]);
            left.hop_count()
                .cmp(&right.hop_count())
                .then_with(|| order(store, left, right))
        });
        chosen.extend(group.into_iter().take(limit));
    }
    let mut chosen = chosen
        .into_iter()
        .map(|index| std::mem::replace(&mut candidates[index], empty()))
        .collect::<Vec<_>>();
    chosen.sort_by(|left, right| order(store, left, right));
    Ok(chosen)
}

fn empty() -> Candidate {
    Candidate {
        vertices: Vec::new(),
        edges: Vec::new(),
        directions: Vec::new(),
        step_hops: Vec::new(),
    }
}
