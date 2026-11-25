use std::collections::{BTreeMap, BTreeSet};

use crate::types::{
    DistanceMetric, HnswGraphNode, HnswGraphState, HnswIndexOptions, NormalizedVectorRecord,
};
use build::GraphBuilder;

#[path = "hnsw/build.rs"]
mod build;
#[path = "hnsw/controlled.rs"]
mod controlled;

#[path = "hnsw/incremental.rs"]
mod incremental;
#[cfg(test)]
#[path = "hnsw/scale_tests.rs"]
mod scale_tests;
#[path = "hnsw/validation.rs"]
mod validation;

pub use controlled::search_graph_with_node_loader;
pub use controlled::{
    search_graph_with_controlled_node_loader, ControlledHnswSearchRequest, HnswGraphRepairReason,
    HnswSearchError,
};
pub use incremental::{
    count_node_heights, GraphMaintainer, HnswLayerCounts, MaintainError, MaintainResult,
    MaintainedGraph, MaintainerSeed,
};
pub use validation::graph_validity_violation;

pub const HNSW_GRAPH_VERSION: u32 = 1;
const MAX_DETERMINISTIC_LAYER: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct HnswCandidate {
    pub id: String,
    pub distance: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HnswSearchResult {
    pub candidates: Vec<HnswCandidate>,
    pub candidate_count: usize,
}

#[derive(Debug, Clone)]
struct SearchCandidate {
    id: String,
    distance: f64,
}

#[derive(Debug, Clone)]
struct GraphDistanceQuery {
    normalized: Vec<f32>,
    raw: Vec<f32>,
}

impl GraphDistanceQuery {
    fn from_query(query: &[f32]) -> Option<Self> {
        let normalized = crate::normalize(query)?.values;
        let raw = query.to_vec();
        Some(Self { normalized, raw })
    }

    fn from_record(record: &NormalizedVectorRecord) -> Self {
        Self::from_parts(record.values.clone(), record.magnitude)
    }

    fn from_parts(normalized: Vec<f32>, magnitude: f64) -> Self {
        Self {
            raw: denormalize(&normalized, magnitude),
            normalized,
        }
    }
}

struct OrderedCandidateSet {
    entries: Vec<SearchCandidate>,
    limit: Option<usize>,
}

impl OrderedCandidateSet {
    fn unbounded() -> Self {
        Self {
            entries: Vec::new(),
            limit: None,
        }
    }

    fn bounded(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            limit: Some(limit.max(1)),
        }
    }

    fn insert(&mut self, candidate: SearchCandidate) {
        let index = self
            .entries
            .binary_search_by(|existing| compare_search_candidates(existing, &candidate))
            .unwrap_or_else(std::convert::identity);
        self.entries.insert(index, candidate);
        if let Some(limit) = self.limit {
            self.entries.truncate(limit);
        }
    }

    fn pop_nearest(&mut self) -> Option<SearchCandidate> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0))
        }
    }

    fn worst_distance(&self) -> f64 {
        self.entries
            .last()
            .map_or(f64::INFINITY, |candidate| candidate.distance)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn into_vec(self) -> Vec<SearchCandidate> {
        self.entries
    }
}

#[must_use]
pub fn graph_fallback_reason(
    graph: Option<&HnswGraphState>,
    metric: DistanceMetric,
    dimensions: usize,
    records: &[NormalizedVectorRecord],
) -> Option<&'static str> {
    let Some(graph) = graph else {
        return Some("missing-graph");
    };
    let current_records = valid_source_records(records, dimensions, metric);
    if current_records.len() != records.len() {
        return Some("incompatible-source");
    }
    let source_fingerprint = crate::normalized_vector_source_fingerprint(&current_records);
    if graph.source_fingerprint != 0 && graph.source_fingerprint != source_fingerprint {
        return Some("stale-source-fingerprint");
    }
    if graph.version != HNSW_GRAPH_VERSION {
        return Some("unsupported-graph-version");
    }
    if graph.metric != metric {
        return Some("incompatible-metric");
    }
    if graph.dimensions != dimensions {
        return Some("incompatible-dimensions");
    }
    if graph.row_count != current_records.len() || graph.nodes.len() != current_records.len() {
        return Some("stale-graph");
    }
    if current_records.is_empty() || graph.nodes.is_empty() {
        return Some("empty-graph");
    }
    let current_by_id = current_records
        .iter()
        .map(|record| (record.id.as_str(), record))
        .collect::<BTreeMap<_, _>>();
    if current_by_id.len() != current_records.len() {
        return Some("duplicate-source-id");
    }
    let node_index = graph_node_index(graph);
    if node_index.len() != graph.nodes.len() {
        return Some("duplicate-node-id");
    }
    if current_by_id.keys().any(|id| !node_index.contains_key(*id)) {
        return Some("missing-current-node");
    }
    let Some(entry_point) = graph.entry_point.as_ref() else {
        return Some("missing-entry-point");
    };
    if !node_index.contains_key(entry_point) {
        return Some("missing-entry-point");
    }
    let mut observed_max_layer = 0usize;
    for node in &graph.nodes {
        let Some(source) = current_by_id.get(node.id.as_str()) else {
            return Some("unknown-node-id");
        };
        if node.vector.len() != dimensions || node.layers.is_empty() {
            return Some("incompatible-node");
        }
        if node.vector != source.values || node.magnitude.to_bits() != source.magnitude.to_bits() {
            return Some("stale-node-vector");
        }
        if node.layers.len() > graph.max_layer.saturating_add(1) {
            return Some("invalid-node-layer");
        }
        observed_max_layer = observed_max_layer.max(node.layers.len().saturating_sub(1));
        for (layer, neighbors) in node.layers.iter().enumerate() {
            for neighbor_id in neighbors {
                let Some(neighbor) = graph_node(graph, &node_index, neighbor_id) else {
                    return Some("unknown-neighbor-id");
                };
                if neighbor.layers.len() <= layer {
                    return Some("invalid-neighbor-layer");
                }
            }
        }
    }
    if observed_max_layer != graph.max_layer {
        return Some("inconsistent-max-layer");
    }
    None
}

#[must_use]
pub fn build_graph(
    mut records: Vec<NormalizedVectorRecord>,
    options: &HnswIndexOptions,
    dimensions: usize,
    metric: DistanceMetric,
) -> HnswGraphState {
    records = valid_source_records(&records, dimensions, metric);
    records.sort_by(|left, right| left.id.cmp(&right.id));
    let source_fingerprint = crate::normalized_vector_source_fingerprint(&records);

    let mut builder = GraphBuilder::new(
        options.m.max(1),
        options.ef_construction.max(1),
        metric,
        source_fingerprint,
    );
    for record in records {
        builder.insert(record);
    }
    builder.finish(dimensions, metric)
}

pub fn search_graph(
    graph: &HnswGraphState,
    query: &[f32],
    options: &HnswIndexOptions,
    limit: usize,
) -> Option<HnswSearchResult> {
    let distance_query = GraphDistanceQuery::from_query(query)?;
    let normalized_query = crate::normalize(query)?;
    let node_index = graph_node_index(graph);
    let entry_point = graph.entry_point.as_ref()?;
    let mut current = entry_point.clone();
    for layer in (1..=graph.max_layer).rev() {
        current = greedy_layer_search(graph, &node_index, &distance_query, &current, layer);
    }

    let ef_search = options.ef_search.max(limit).max(1);
    let mut candidates =
        search_graph_layer(graph, &node_index, &distance_query, &current, 0, ef_search);
    if candidates.is_empty() {
        let entry = graph_node(graph, &node_index, entry_point)?;
        candidates.push(SearchCandidate {
            id: entry.id.clone(),
            distance: graph_distance(graph.metric, &distance_query, entry),
        });
    }
    let candidate_count = candidates.len();
    let mut exact = candidates
        .into_iter()
        .filter_map(|candidate| {
            let node = graph_node(graph, &node_index, &candidate.id)?;
            Some(HnswCandidate {
                id: candidate.id,
                distance: exact_distance(graph.metric, query, &normalized_query.values, node),
            })
        })
        .collect::<Vec<_>>();
    exact.sort_by(compare_hnsw_candidates);
    exact.truncate(limit.max(1));
    Some(HnswSearchResult {
        candidates: exact,
        candidate_count,
    })
}

pub fn search(
    query: &[f32],
    candidates: impl IntoIterator<Item = (String, Vec<f32>)>,
    limit: usize,
    metric: fn(&[f32], &[f32]) -> f64,
) -> Vec<HnswCandidate> {
    let mut scored = candidates
        .into_iter()
        .filter(|(_, vector)| vector.len() == query.len())
        .map(|(id, vector)| HnswCandidate {
            distance: metric(query, &vector),
            id,
        })
        .collect::<Vec<_>>();
    scored.sort_by(compare_hnsw_candidates);
    scored.truncate(limit.max(1));
    scored
}

fn graph_node_index(graph: &HnswGraphState) -> BTreeMap<String, usize> {
    graph
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect()
}

fn graph_node<'a>(
    graph: &'a HnswGraphState,
    index: &BTreeMap<String, usize>,
    id: &str,
) -> Option<&'a HnswGraphNode> {
    index
        .get(id)
        .and_then(|node_index| graph.nodes.get(*node_index))
}

fn greedy_layer_search(
    graph: &HnswGraphState,
    index: &BTreeMap<String, usize>,
    query: &GraphDistanceQuery,
    entry_point: &str,
    layer: usize,
) -> String {
    let mut current = entry_point.to_string();
    loop {
        let Some(current_node) = graph_node(graph, index, &current) else {
            return current;
        };
        let current_distance = graph_distance(graph.metric, query, current_node);
        let Some(neighbors) = current_node.layers.get(layer) else {
            return current;
        };
        let best = neighbors
            .iter()
            .filter_map(|neighbor| {
                let node = graph_node(graph, index, neighbor)?;
                Some((neighbor, graph_distance(graph.metric, query, node)))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1).then_with(|| left.0.cmp(right.0)));
        let Some((best_id, best_distance)) = best else {
            return current;
        };
        if best_distance >= current_distance {
            return current;
        }
        current.clone_from(best_id);
    }
}

fn search_graph_layer(
    graph: &HnswGraphState,
    index: &BTreeMap<String, usize>,
    query: &GraphDistanceQuery,
    entry_point: &str,
    layer: usize,
    ef: usize,
) -> Vec<SearchCandidate> {
    let Some(entry) = graph_node(graph, index, entry_point) else {
        return Vec::new();
    };
    let mut visited = BTreeSet::new();
    let entry_candidate = SearchCandidate {
        id: entry.id.clone(),
        distance: graph_distance(graph.metric, query, entry),
    };
    visited.insert(entry.id.clone());
    let mut candidates = OrderedCandidateSet::unbounded();
    candidates.insert(entry_candidate.clone());
    let mut nearest = OrderedCandidateSet::bounded(ef);
    nearest.insert(entry_candidate);

    while let Some(candidate) = pop_nearest(&mut candidates) {
        let worst_distance = nearest.worst_distance();
        if nearest.len() >= ef && candidate.distance > worst_distance {
            break;
        }
        let Some(node) = graph_node(graph, index, &candidate.id) else {
            continue;
        };
        let Some(neighbors) = node.layers.get(layer) else {
            continue;
        };
        for neighbor_id in neighbors {
            if !visited.insert(neighbor_id.clone()) {
                continue;
            }
            let Some(neighbor) = graph_node(graph, index, neighbor_id) else {
                continue;
            };
            let distance = graph_distance(graph.metric, query, neighbor);
            if nearest.len() < ef || distance < worst_distance {
                let next = SearchCandidate {
                    id: neighbor.id.clone(),
                    distance,
                };
                candidates.insert(next.clone());
                nearest.insert(next);
            }
        }
    }
    nearest.into_vec()
}

fn graph_distance(metric: DistanceMetric, query: &GraphDistanceQuery, node: &HnswGraphNode) -> f64 {
    match metric {
        DistanceMetric::Cosine => {
            crate::cosine_distance_from_normalized_query(&query.normalized, &node.vector)
        }
        DistanceMetric::Dot => {
            crate::dot_distance_from_normalized_target(&query.raw, &node.vector, node.magnitude)
        }
        DistanceMetric::L2 => l2_to_stored(&query.raw, &node.vector, node.magnitude),
    }
}

fn exact_distance(
    metric: DistanceMetric,
    query: &[f32],
    normalized_query: &[f32],
    node: &HnswGraphNode,
) -> f64 {
    match metric {
        DistanceMetric::Cosine => {
            crate::cosine_distance_from_normalized_query(normalized_query, &node.vector)
        }
        DistanceMetric::Dot => {
            crate::dot_distance_from_normalized_target(query, &node.vector, node.magnitude)
        }
        DistanceMetric::L2 => l2_to_stored(query, &node.vector, node.magnitude),
    }
}

thread_local! {
    static DENORMALIZED_SCRATCH: std::cell::RefCell<Vec<f32>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// L2 distance from `query` to a stored normalized vector, denormalized into a
/// reused per-thread buffer instead of a fresh allocation per comparison.
fn l2_to_stored(query: &[f32], vector: &[f32], magnitude: f64) -> f64 {
    DENORMALIZED_SCRATCH.with(|scratch| {
        let mut target = scratch.borrow_mut();
        target.clear();
        target.extend(vector.iter().map(|value| {
            crate::denormalize_f32_component(*value, magnitude).unwrap_or(f32::INFINITY)
        }));
        crate::l2_distance(query, &target)
    })
}

fn denormalize(vector: &[f32], magnitude: f64) -> Vec<f32> {
    vector
        .iter()
        .map(|value| crate::denormalize_f32_component(*value, magnitude).unwrap_or(f32::INFINITY))
        .collect()
}

fn valid_source_records(
    records: &[NormalizedVectorRecord],
    dimensions: usize,
    metric: DistanceMetric,
) -> Vec<NormalizedVectorRecord> {
    let mut valid = records
        .iter()
        .filter(|record| {
            record.payload_available
                && record.dimensions == dimensions
                && record.metric == metric
                && record.values.len() == dimensions
                && record.normalization_version
                    == NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION
        })
        .cloned()
        .collect::<Vec<_>>();
    valid.sort_by(|left, right| left.id.cmp(&right.id));
    valid
}

fn deterministic_level(id: &str) -> usize {
    let hash = stable_id_hash(id);
    let zero_bit_pairs = hash.trailing_zeros() / 2;
    usize::try_from(zero_bit_pairs)
        .unwrap_or(MAX_DETERMINISTIC_LAYER)
        .min(MAX_DETERMINISTIC_LAYER)
}

fn stable_id_hash(id: &str) -> u64 {
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    for byte in id.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x0100_0000_01b3);
    }
    state
}

fn pop_nearest(candidates: &mut OrderedCandidateSet) -> Option<SearchCandidate> {
    candidates.pop_nearest()
}

fn compare_search_candidates(
    left: &SearchCandidate,
    right: &SearchCandidate,
) -> std::cmp::Ordering {
    left.distance
        .total_cmp(&right.distance)
        .then_with(|| left.id.cmp(&right.id))
}

fn compare_hnsw_candidates(left: &HnswCandidate, right: &HnswCandidate) -> std::cmp::Ordering {
    left.distance
        .total_cmp(&right.distance)
        .then_with(|| left.id.cmp(&right.id))
}

#[cfg(test)]
mod tests {
    use super::{build_graph, graph_distance, search, search_graph, GraphDistanceQuery};
    use crate::types::{DistanceMetric, HnswGraphNode, HnswIndexOptions, NormalizedVectorRecord};

    #[test]
    fn should_return_exactly_ranked_hnsw_candidates() {
        // Arrange
        let query = vec![0.0, 0.0];
        let candidates = vec![
            ("far".to_string(), vec![2.0, 0.0]),
            ("near".to_string(), vec![1.0, 0.0]),
            ("wrong-dim".to_string(), vec![0.0]),
        ];

        // Act
        let selected = search(&query, candidates, 2, crate::l2_distance);

        // Assert
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].id, "near");
        assert_eq!(selected[1].id, "far");
    }

    #[test]
    fn should_search_deterministic_graph_after_build() {
        // Arrange
        let records = ["near", "middle", "far"]
            .into_iter()
            .zip([
                vec![1.0, 0.0, 0.0],
                vec![0.7, 0.7, 0.0],
                vec![-1.0, 0.0, 0.0],
            ])
            .map(|(id, values)| NormalizedVectorRecord {
                built_generation: 0,
                collection: "docs".to_string(),
                field: "embedding".to_string(),
                id: id.to_string(),
                dimensions: 3,
                metric: DistanceMetric::L2,
                normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
                payload_available: true,
                magnitude: 1.0,
                values,
            })
            .collect::<Vec<_>>();
        let options = HnswIndexOptions {
            version: 1,
            m: 2,
            ef_construction: 4,
            ef_search: 2,
        };
        let graph = build_graph(records, &options, 3, DistanceMetric::L2);

        // Act
        let result = search_graph(&graph, &[1.0, 0.0, 0.0], &options, 1).expect("search graph");

        // Assert
        assert_eq!(result.candidates[0].id, "near");
        assert!(result.candidate_count <= 2);
    }

    #[test]
    fn should_use_metric_specific_graph_distances() {
        // Arrange
        let query = GraphDistanceQuery {
            normalized: vec![1.0, 0.0],
            raw: vec![2.0, 0.0],
        };
        let small_aligned = HnswGraphNode {
            id: "small".to_string(),
            vector: vec![1.0, 0.0],
            magnitude: 1.0,
            layers: vec![Vec::new()],
        };
        let large_aligned = HnswGraphNode {
            id: "large".to_string(),
            vector: vec![1.0, 0.0],
            magnitude: 10.0,
            layers: vec![Vec::new()],
        };
        let nearby_l2 = HnswGraphNode {
            id: "nearby".to_string(),
            vector: vec![1.0, 0.0],
            magnitude: 2.0,
            layers: vec![Vec::new()],
        };
        let far_l2 = HnswGraphNode {
            id: "far".to_string(),
            vector: vec![1.0, 0.0],
            magnitude: 20.0,
            layers: vec![Vec::new()],
        };

        // Act
        let dot_large = graph_distance(DistanceMetric::Dot, &query, &large_aligned);
        let dot_small = graph_distance(DistanceMetric::Dot, &query, &small_aligned);
        let cosine_large = graph_distance(DistanceMetric::Cosine, &query, &large_aligned);
        let cosine_small = graph_distance(DistanceMetric::Cosine, &query, &small_aligned);
        let l2_near = graph_distance(DistanceMetric::L2, &query, &nearby_l2);
        let l2_far = graph_distance(DistanceMetric::L2, &query, &far_l2);

        // Assert
        assert_eq!(dot_large, -20.0);
        assert_eq!(dot_small, -2.0);
        assert_eq!(cosine_large, 0.0);
        assert_eq!(cosine_small, 0.0);
        assert_eq!(l2_near, 0.0);
        assert_eq!(l2_far, 18.0);
    }
}
