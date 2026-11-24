//! The contract's total order on element references and paths.
//!
//! "Compare paths lexicographically by the interleaved sequence
//! `V0, E0, direction0, V1, E1, direction1, V2, ...`: element references order
//! by durable database ID, graph ID, kind, mapping ID and key tuple; key
//! components order first by declared type (smallint, integer, bigint, text,
//! uuid), then by that type's value (C byte order for text and unsigned
//! 16-byte order for UUID). Compare direction after each edge identity,
//! forward before reverse, and the shorter sequence first when one is a
//! prefix."
use std::cmp::Ordering;

use super::model::{ElementRef, Path};

/// Orders two element references by database, graph, kind, mapping and key
/// tuple. Key tuples compare component by component (type, then value), the
/// shorter tuple first when one is a prefix.
#[must_use]
pub fn compare_element_refs(left: &ElementRef, right: &ElementRef) -> Ordering {
    left.database_id
        .cmp(&right.database_id)
        .then_with(|| left.graph_id.cmp(&right.graph_id))
        .then_with(|| left.kind.cmp(&right.kind))
        .then_with(|| left.mapping_id.cmp(&right.mapping_id))
        .then_with(|| left.key.cmp(&right.key))
}

/// Orders two paths by their interleaved vertex, edge and direction sequence.
#[must_use]
pub fn compare_paths(left: &Path, right: &Path) -> Ordering {
    let steps = left.vertices.len().min(right.vertices.len());
    for index in 0..steps {
        let order = compare_element_refs(&left.vertices[index], &right.vertices[index]);
        if order != Ordering::Equal {
            return order;
        }
        // The edge after vertex `index` exists only when both paths go on.
        let (Some(left_edge), Some(right_edge)) = (left.edges.get(index), right.edges.get(index))
        else {
            break;
        };
        let order = compare_element_refs(left_edge, right_edge)
            .then_with(|| left.directions.get(index).cmp(&right.directions.get(index)));
        if order != Ordering::Equal {
            return order;
        }
    }
    left.edges.len().cmp(&right.edges.len())
}

impl PartialOrd for ElementRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ElementRef {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_element_refs(self, other)
    }
}

impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Path {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_paths(self, other)
    }
}
