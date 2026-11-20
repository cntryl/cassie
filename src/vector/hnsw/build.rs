//! Full HNSW graph construction over dense node indexes.
//!
//! Records arrive sorted by id, so a node's dense index order equals its id
//! order. Every tie the string-keyed algorithm broke by id is therefore broken
//! by index here, which keeps the graph identical to the persisted format
//! while node lookup, visited tracking and candidate ordering avoid string
//! hashing, string cloning and per-distance allocation.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use super::{deterministic_level, GraphDistanceQuery, HNSW_GRAPH_VERSION};
use crate::embeddings::{DistanceMetric, HnswGraphNode, HnswGraphState, NormalizedVectorRecord};

struct DenseNode {
    id: String,
    magnitude: f64,
    /// `normalized` holds the stored unit vector, `raw` its denormalized form.
    query: GraphDistanceQuery,
    layers: Vec<Vec<u32>>,
}

#[derive(Clone, Copy, PartialEq)]
struct Candidate {
    distance: f64,
    index: u32,
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then_with(|| self.index.cmp(&other.index))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Generation-stamped visited set reused across searches.
#[derive(Default)]
struct Visited {
    stamps: Vec<u32>,
    generation: u32,
}

impl Visited {
    fn begin(&mut self, node_count: usize) {
        if self.stamps.len() < node_count {
            self.stamps.resize(node_count, 0);
        }
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.stamps.fill(0);
            self.generation = 1;
        }
    }

    /// Marks `index` visited; returns whether it was newly visited.
    fn insert(&mut self, index: u32) -> bool {
        let slot = &mut self.stamps[index as usize];
        if *slot == self.generation {
            false
        } else {
            *slot = self.generation;
            true
        }
    }
}

pub(super) struct GraphBuilder {
    nodes: Vec<DenseNode>,
    entry_point: Option<u32>,
    max_layer: usize,
    m: usize,
    ef_construction: usize,
    metric: DistanceMetric,
    source_fingerprint: u64,
    visited: Visited,
}

fn distance(metric: DistanceMetric, query: &GraphDistanceQuery, node: &DenseNode) -> f64 {
    match metric {
        DistanceMetric::Cosine => crate::vector::cosine_distance_from_normalized_query(
            &query.normalized,
            &node.query.normalized,
        ),
        DistanceMetric::Dot => crate::vector::dot_distance_from_normalized_target(
            &query.raw,
            &node.query.normalized,
            node.magnitude,
        ),
        DistanceMetric::L2 => crate::vector::l2_distance(&query.raw, &node.query.raw),
    }
}

impl GraphBuilder {
    pub(super) fn new(
        m: usize,
        ef_construction: usize,
        metric: DistanceMetric,
        source_fingerprint: u64,
    ) -> Self {
        Self {
            nodes: Vec::new(),
            entry_point: None,
            max_layer: 0,
            m,
            ef_construction,
            metric,
            source_fingerprint,
            visited: Visited::default(),
        }
    }

    /// Inserts a record whose id sorts after every id inserted before it.
    pub(super) fn insert(&mut self, record: NormalizedVectorRecord) {
        let level = deterministic_level(&record.id);
        let mut layers: Vec<Vec<u32>> = vec![Vec::new(); level.saturating_add(1)];
        let query = GraphDistanceQuery::from_record(&record);
        if let Some(mut current) = self.entry_point {
            for layer in (level.saturating_add(1)..=self.max_layer).rev() {
                current = self.greedy_layer_search(&query, current, layer);
            }
            for layer in (0..=level.min(self.max_layer)).rev() {
                let candidates = self.search_layer(&query, current, layer, self.ef_construction);
                if let Some(nearest) = candidates.first() {
                    current = nearest.index;
                }
                layers[layer] = candidates
                    .into_iter()
                    .take(self.m)
                    .map(|candidate| candidate.index)
                    .collect();
            }
        }
        let index = u32::try_from(self.nodes.len()).expect("node count fits u32");
        let selected = layers.clone();
        self.nodes.push(DenseNode {
            id: record.id,
            magnitude: record.magnitude,
            query,
            layers,
        });
        for (layer, neighbors) in selected.into_iter().enumerate() {
            for neighbor in neighbors {
                self.add_neighbor(neighbor, index, layer);
            }
        }
        if level > self.max_layer || self.entry_point.is_none() {
            self.max_layer = level;
            self.entry_point = Some(index);
        }
    }

    fn add_neighbor(&mut self, target: u32, neighbor: u32, layer: usize) {
        let links = &mut self.nodes[target as usize].layers;
        if links.len() <= layer {
            return;
        }
        if !links[layer].contains(&neighbor) {
            links[layer].push(neighbor);
        }
        self.prune_neighbors(target, layer);
    }

    /// Keeps the `m` links of `target` closest to it. A dropped neighbor drops
    /// its link back, so links stay symmetric and deletion only has to repair
    /// the deleted node's own neighbors.
    fn prune_neighbors(&mut self, target: u32, layer: usize) {
        let target_node = &self.nodes[target as usize];
        let mut ranked = target_node.layers[layer]
            .iter()
            .map(|&index| Candidate {
                distance: distance(self.metric, &target_node.query, &self.nodes[index as usize]),
                index,
            })
            .collect::<Vec<_>>();
        ranked.sort_unstable();
        ranked.dedup_by_key(|candidate| candidate.index);
        let dropped = ranked.split_off(self.m.min(ranked.len()));
        self.nodes[target as usize].layers[layer] = ranked
            .into_iter()
            .map(|candidate| candidate.index)
            .collect();
        for candidate in dropped {
            if let Some(links) = self.nodes[candidate.index as usize].layers.get_mut(layer) {
                links.retain(|&link| link != target);
            }
        }
    }

    fn greedy_layer_search(&self, query: &GraphDistanceQuery, entry: u32, layer: usize) -> u32 {
        let mut current = entry;
        loop {
            let current_node = &self.nodes[current as usize];
            let current_distance = distance(self.metric, query, current_node);
            let Some(neighbors) = current_node.layers.get(layer) else {
                return current;
            };
            let best = neighbors
                .iter()
                .map(|&index| Candidate {
                    distance: distance(self.metric, query, &self.nodes[index as usize]),
                    index,
                })
                .min();
            let Some(best) = best else {
                return current;
            };
            if best.distance >= current_distance {
                return current;
            }
            current = best.index;
        }
    }

    /// Returns up to `ef` nearest candidates in ascending distance order.
    fn search_layer(
        &mut self,
        query: &GraphDistanceQuery,
        entry: u32,
        layer: usize,
        ef: usize,
    ) -> Vec<Candidate> {
        let ef = ef.max(1);
        self.visited.begin(self.nodes.len());
        let entry_candidate = Candidate {
            distance: distance(self.metric, query, &self.nodes[entry as usize]),
            index: entry,
        };
        self.visited.insert(entry);
        let mut candidates = BinaryHeap::from([Reverse(entry_candidate)]);
        let mut nearest = BinaryHeap::from([entry_candidate]);

        while let Some(Reverse(candidate)) = candidates.pop() {
            let worst_distance = nearest.peek().map_or(f64::INFINITY, |worst| worst.distance);
            if nearest.len() >= ef && candidate.distance > worst_distance {
                break;
            }
            let Some(neighbors) = self.nodes[candidate.index as usize].layers.get(layer) else {
                continue;
            };
            for &neighbor in neighbors {
                if !self.visited.insert(neighbor) {
                    continue;
                }
                let neighbor_distance =
                    distance(self.metric, query, &self.nodes[neighbor as usize]);
                if nearest.len() < ef || neighbor_distance < worst_distance {
                    let next = Candidate {
                        distance: neighbor_distance,
                        index: neighbor,
                    };
                    candidates.push(Reverse(next));
                    nearest.push(next);
                    if nearest.len() > ef {
                        nearest.pop();
                    }
                }
            }
        }
        nearest.into_sorted_vec()
    }

    pub(super) fn finish(self, dimensions: usize, metric: DistanceMetric) -> HnswGraphState {
        let ids = self
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        let nodes = self
            .nodes
            .into_iter()
            .map(|node| HnswGraphNode {
                id: node.id,
                vector: node.query.normalized,
                magnitude: node.magnitude,
                layers: node
                    .layers
                    .into_iter()
                    .map(|links| {
                        links
                            .into_iter()
                            .map(|index| ids[index as usize].clone())
                            .collect()
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        HnswGraphState {
            version: HNSW_GRAPH_VERSION,
            source_fingerprint: self.source_fingerprint,
            row_count: nodes.len(),
            dimensions,
            metric,
            entry_point: self.entry_point.map(|index| ids[index as usize].clone()),
            max_layer: self.max_layer,
            nodes,
        }
    }
}
