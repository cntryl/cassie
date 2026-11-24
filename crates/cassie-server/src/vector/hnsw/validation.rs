//! Structural validation of a persisted HNSW graph.

use std::collections::{BTreeMap, BTreeSet};

use crate::embeddings::HnswGraphState;

/// Returns the first structural violation of `graph`, or `None` when it is a
/// valid navigable graph.
///
/// A valid graph has: a row count equal to its node count; an entry point that
/// exists, sits on the highest layer, and matches `max_layer`; no node taller
/// than `max_layer`; neighbor lists of at most `max_neighbors` entries that
/// name distinct existing nodes other than the owner; neighbors tall enough to
/// own the layer they are listed on; and links that are symmetric, which is
/// what lets a node be deleted by repairing only its own neighbors.
#[must_use]
pub fn graph_validity_violation(graph: &HnswGraphState, max_neighbors: usize) -> Option<String> {
    if graph.row_count != graph.nodes.len() {
        return Some(format!(
            "row_count {} differs from node count {}",
            graph.row_count,
            graph.nodes.len()
        ));
    }
    let by_id = graph
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    if by_id.len() != graph.nodes.len() {
        return Some("duplicate node id".to_string());
    }
    let Some(entry_id) = graph.entry_point.as_deref() else {
        return (!graph.nodes.is_empty()).then(|| "missing entry point".to_string());
    };
    let Some(entry) = by_id.get(entry_id) else {
        return Some(format!("entry point {entry_id} is not a node"));
    };
    let observed_max = graph
        .nodes
        .iter()
        .map(|node| node.layers.len().saturating_sub(1))
        .max()
        .unwrap_or(0);
    if entry.layers.len().saturating_sub(1) != graph.max_layer || observed_max != graph.max_layer {
        return Some(format!(
            "entry point is not on max layer {} (entry {}, tallest node {observed_max})",
            graph.max_layer,
            entry.layers.len().saturating_sub(1)
        ));
    }
    graph
        .nodes
        .iter()
        .find_map(|node| node_violation(node, &by_id, max_neighbors))
}

fn node_violation(
    node: &crate::embeddings::HnswGraphNode,
    by_id: &BTreeMap<&str, &crate::embeddings::HnswGraphNode>,
    max_neighbors: usize,
) -> Option<String> {
    if node.layers.is_empty() {
        return Some(format!("node {} has no layers", node.id));
    }
    for (layer, neighbors) in node.layers.iter().enumerate() {
        if neighbors.len() > max_neighbors {
            return Some(format!(
                "node {} layer {layer} has {} neighbors, above {max_neighbors}",
                node.id,
                neighbors.len()
            ));
        }
        let mut seen = BTreeSet::new();
        for neighbor_id in neighbors {
            if neighbor_id == &node.id || !seen.insert(neighbor_id.as_str()) {
                return Some(format!(
                    "node {} layer {layer} repeats or self-links {neighbor_id}",
                    node.id
                ));
            }
            let Some(neighbor) = by_id.get(neighbor_id.as_str()) else {
                return Some(format!(
                    "node {} layer {layer} names unknown neighbor {neighbor_id}",
                    node.id
                ));
            };
            if neighbor.layers.len() <= layer {
                return Some(format!(
                    "neighbor {neighbor_id} of {} is too short for layer {layer}",
                    node.id
                ));
            }
            if !neighbor.layers[layer].contains(&node.id) {
                return Some(format!(
                    "link {} -> {neighbor_id} on layer {layer} is not symmetric",
                    node.id
                ));
            }
        }
    }
    None
}
