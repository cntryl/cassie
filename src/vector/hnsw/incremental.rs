//! Localized HNSW maintenance over a persisted graph.
//!
//! [`GraphMaintainer`] applies document-level changes to a graph whose nodes
//! live behind a point-read loader. It reads only the nodes an insertion search
//! or a deletion repair visits, keeps every modified node in memory, and hands
//! back exactly the nodes whose records changed. Nothing proportional to the
//! unrelated graph is read or rewritten.
//!
//! The graph keeps one invariant that makes deletion local: links are
//! symmetric. Insertion links the new node both ways and, when a neighbor
//! overflows `m`, the neighbor drops its farthest link *and* that node drops
//! the link back. Deletion therefore finds every in-link in the deleted node's
//! own lists, unlinks them, and reconnects the orphaned neighbors to one
//! another. When the entry point is deleted, the tallest neighbor on its
//! highest populated layer becomes the entry point.
//!
//! Any stored state that contradicts the graph (a missing neighbor, a neighbor
//! too short for a layer, an entry point that cannot be re-elected) surfaces as
//! [`MaintainError::Inconsistent`], so the caller repairs by a full rebuild
//! instead of failing the user's write.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    deterministic_level, graph_distance, pop_nearest, GraphDistanceQuery, OrderedCandidateSet,
    SearchCandidate,
};
use crate::app::CassieError;
use crate::embeddings::{DistanceMetric, HnswGraphNode, HnswIndexOptions, NormalizedVectorRecord};
use crate::vector::source_fingerprint::{
    fold_record_fingerprint, normalized_vector_record_fingerprint,
};

/// Why localized maintenance stopped.
#[derive(Debug)]
pub(crate) enum MaintainError {
    /// Reading a node failed; the write itself cannot proceed.
    Store(CassieError),
    /// The persisted graph contradicts itself; repair it by a full rebuild.
    Inconsistent(&'static str),
}

impl From<CassieError> for MaintainError {
    fn from(error: CassieError) -> Self {
        Self::Store(error)
    }
}

/// Persisted graph header a maintenance run starts from.
pub(crate) struct MaintainerSeed {
    pub(crate) entry_point: Option<String>,
    pub(crate) max_layer: usize,
    pub(crate) row_count: usize,
    pub(crate) source_fingerprint: u64,
    pub(crate) metric: DistanceMetric,
    pub(crate) options: HnswIndexOptions,
}

/// Result of a maintenance run: the records to write and the new header.
#[derive(Debug)]
pub(crate) struct MaintainedGraph {
    pub(crate) puts: Vec<HnswGraphNode>,
    pub(crate) deletes: Vec<String>,
    pub(crate) entry_point: Option<String>,
    pub(crate) max_layer: usize,
    pub(crate) row_count: usize,
    pub(crate) source_fingerprint: u64,
    pub(crate) nodes_read: usize,
}

pub(crate) struct GraphMaintainer<L> {
    load: L,
    cache: BTreeMap<String, Option<HnswGraphNode>>,
    dirty: BTreeSet<String>,
    nodes_read: usize,
    entry_point: Option<String>,
    max_layer: usize,
    row_count: usize,
    source_fingerprint: u64,
    metric: DistanceMetric,
    m: usize,
    ef_construction: usize,
}

type Outcome<T> = Result<T, MaintainError>;

fn inconsistent<T>(reason: &'static str) -> Outcome<T> {
    Err(MaintainError::Inconsistent(reason))
}

impl<L> GraphMaintainer<L>
where
    L: FnMut(&str) -> Result<Option<HnswGraphNode>, MaintainError>,
{
    pub(crate) fn new(seed: MaintainerSeed, load: L) -> Self {
        Self {
            load,
            cache: BTreeMap::new(),
            dirty: BTreeSet::new(),
            nodes_read: 0,
            entry_point: seed.entry_point,
            max_layer: seed.max_layer,
            row_count: seed.row_count,
            source_fingerprint: seed.source_fingerprint,
            metric: seed.metric,
            m: seed.options.m.max(1),
            ef_construction: seed.options.ef_construction.max(1),
        }
    }

    /// The stored vector of `id`, when it is a node.
    pub(crate) fn stored_vector(&mut self, id: &str) -> Outcome<Option<(Vec<f32>, f64)>> {
        Ok(self
            .node(id)?
            .map(|node| (node.vector.clone(), node.magnitude)))
    }

    /// Ends the run and returns what must be written.
    pub(crate) fn finish(self) -> MaintainedGraph {
        let mut puts = Vec::new();
        let mut deletes = Vec::new();
        for id in self.dirty {
            match self.cache.get(&id) {
                Some(Some(node)) => puts.push(node.clone()),
                _ => deletes.push(id),
            }
        }
        MaintainedGraph {
            puts,
            deletes,
            entry_point: self.entry_point,
            max_layer: self.max_layer,
            row_count: self.row_count,
            source_fingerprint: self.source_fingerprint,
            nodes_read: self.nodes_read,
        }
    }

    fn node(&mut self, id: &str) -> Outcome<Option<&HnswGraphNode>> {
        if !self.cache.contains_key(id) {
            let loaded = (self.load)(id)?;
            self.nodes_read = self.nodes_read.saturating_add(1);
            self.cache.insert(id.to_string(), loaded);
        }
        Ok(self.cache.get(id).and_then(Option::as_ref))
    }

    fn require(&mut self, id: &str) -> Outcome<&HnswGraphNode> {
        match self.node(id)? {
            Some(node) => Ok(node),
            None => inconsistent("unknown-neighbor-id"),
        }
    }

    fn node_mut(&mut self, id: &str) -> Outcome<&mut HnswGraphNode> {
        self.require(id)?;
        self.dirty.insert(id.to_string());
        match self.cache.get_mut(id).and_then(Option::as_mut) {
            Some(node) => Ok(node),
            None => inconsistent("unknown-neighbor-id"),
        }
    }

    fn query_of(&mut self, id: &str) -> Outcome<GraphDistanceQuery> {
        let node = self.require(id)?;
        Ok(GraphDistanceQuery::from_parts(
            node.vector.clone(),
            node.magnitude,
        ))
    }

    fn distance_to(&mut self, query: &GraphDistanceQuery, id: &str) -> Outcome<f64> {
        let metric = self.metric;
        let node = self.require(id)?;
        Ok(graph_distance(metric, query, node))
    }

    fn links(&mut self, id: &str, layer: usize) -> Outcome<Vec<String>> {
        match self.require(id)?.layers.get(layer) {
            Some(links) => Ok(links.clone()),
            None => inconsistent("invalid-neighbor-layer"),
        }
    }

    fn remove_link(&mut self, from: &str, to: &str, layer: usize) -> Outcome<()> {
        if !self.links(from, layer)?.iter().any(|link| link == to) {
            return Ok(());
        }
        self.node_mut(from)?.layers[layer].retain(|link| link != to);
        Ok(())
    }

    fn push_link(&mut self, from: &str, to: &str, layer: usize) -> Outcome<()> {
        if self.links(from, layer)?.iter().any(|link| link == to) {
            return Ok(());
        }
        self.node_mut(from)?.layers[layer].push(to.to_string());
        Ok(())
    }

    /// Keeps the `m` links of `target` closest to it. A dropped neighbor drops
    /// its link back, so links stay symmetric.
    fn prune(&mut self, target: &str, layer: usize) -> Outcome<()> {
        let links = self.links(target, layer)?;
        if links.len() <= self.m {
            return Ok(());
        }
        let query = self.query_of(target)?;
        let mut ranked = Vec::with_capacity(links.len());
        for link in links {
            ranked.push((self.distance_to(&query, &link)?, link));
        }
        ranked.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });
        let dropped = ranked.split_off(self.m);
        self.node_mut(target)?.layers[layer] = ranked.into_iter().map(|(_, id)| id).collect();
        for (_, dropped_id) in dropped {
            self.remove_link(&dropped_id, target, layer)?;
        }
        Ok(())
    }

    fn greedy_layer_search(
        &mut self,
        query: &GraphDistanceQuery,
        entry: &str,
        layer: usize,
    ) -> Outcome<String> {
        let mut current = entry.to_string();
        let mut current_distance = self.distance_to(query, &current)?;
        loop {
            let Some(neighbors) = self.require(&current)?.layers.get(layer).cloned() else {
                return Ok(current);
            };
            let mut best: Option<(f64, String)> = None;
            for neighbor in neighbors {
                let distance = self.distance_to(query, &neighbor)?;
                let better = best.as_ref().is_none_or(|(best_distance, best_id)| {
                    distance
                        .total_cmp(best_distance)
                        .then_with(|| neighbor.cmp(best_id))
                        .is_lt()
                });
                if better {
                    best = Some((distance, neighbor));
                }
            }
            let Some((best_distance, best_id)) = best else {
                return Ok(current);
            };
            if best_distance >= current_distance {
                return Ok(current);
            }
            current = best_id;
            current_distance = best_distance;
        }
    }

    fn search_layer(
        &mut self,
        query: &GraphDistanceQuery,
        entry: &str,
        layer: usize,
        ef: usize,
    ) -> Outcome<Vec<SearchCandidate>> {
        let entry_candidate = SearchCandidate {
            id: entry.to_string(),
            distance: self.distance_to(query, entry)?,
        };
        let mut visited = BTreeSet::from([entry.to_string()]);
        let mut candidates = OrderedCandidateSet::unbounded();
        candidates.insert(entry_candidate.clone());
        let mut nearest = OrderedCandidateSet::bounded(ef);
        nearest.insert(entry_candidate);
        while let Some(candidate) = pop_nearest(&mut candidates) {
            let worst_distance = nearest.worst_distance();
            if nearest.len() >= ef && candidate.distance > worst_distance {
                break;
            }
            let Some(neighbors) = self.require(&candidate.id)?.layers.get(layer).cloned() else {
                continue;
            };
            for neighbor in neighbors {
                if !visited.insert(neighbor.clone()) {
                    continue;
                }
                let distance = self.distance_to(query, &neighbor)?;
                if nearest.len() < ef || distance < worst_distance {
                    let next = SearchCandidate {
                        id: neighbor,
                        distance,
                    };
                    candidates.insert(next.clone());
                    nearest.insert(next);
                }
            }
        }
        Ok(nearest.into_vec())
    }

    /// Inserts `record` as a new node linked both ways to its nearest
    /// neighbors on every layer it owns.
    pub(crate) fn insert(&mut self, record: &NormalizedVectorRecord) -> Outcome<()> {
        if self.node(&record.id)?.is_some() {
            return inconsistent("duplicate-node-id");
        }
        let level = deterministic_level(&record.id);
        let query = GraphDistanceQuery::from_record(record);
        let mut layers = vec![Vec::new(); level.saturating_add(1)];
        if let Some(entry) = self.entry_point.clone() {
            if self.require(&entry)?.layers.len() <= self.max_layer {
                return inconsistent("entry-point-layer");
            }
            let mut current = entry;
            for layer in (level.saturating_add(1)..=self.max_layer).rev() {
                current = self.greedy_layer_search(&query, &current, layer)?;
            }
            for layer in (0..=level.min(self.max_layer)).rev() {
                let candidates =
                    self.search_layer(&query, &current, layer, self.ef_construction)?;
                if let Some(nearest) = candidates.first() {
                    current.clone_from(&nearest.id);
                }
                layers[layer] = candidates
                    .into_iter()
                    .take(self.m)
                    .map(|candidate| candidate.id)
                    .collect();
            }
        }
        let selected = layers.clone();
        self.cache.insert(
            record.id.clone(),
            Some(HnswGraphNode {
                id: record.id.clone(),
                vector: record.values.clone(),
                magnitude: record.magnitude,
                layers,
            }),
        );
        self.dirty.insert(record.id.clone());
        for (layer, neighbors) in selected.into_iter().enumerate() {
            for neighbor in neighbors {
                self.push_link(&neighbor, &record.id, layer)?;
                self.prune(&neighbor, layer)?;
            }
        }
        if self.entry_point.is_none() || level > self.max_layer {
            self.max_layer = level;
            self.entry_point = Some(record.id.clone());
        }
        self.row_count = self.row_count.saturating_add(1);
        self.fold(&record.id, &record.values, record.magnitude);
        Ok(())
    }

    /// Deletes `id`, reconnecting its neighbors to one another. Returns whether
    /// the node existed.
    pub(crate) fn remove(&mut self, id: &str) -> Outcome<bool> {
        let Some(removed) = self.node(id)?.cloned() else {
            return Ok(false);
        };
        if self.row_count == 0 {
            return inconsistent("row-count");
        }
        for (layer, links) in removed.layers.iter().enumerate() {
            let mut ring = links.clone();
            ring.sort();
            for neighbor in &ring {
                self.remove_link(neighbor, id, layer)?;
            }
            self.reconnect(&ring, layer)?;
        }
        if self.entry_point.as_deref() == Some(id) {
            self.reelect_entry_point(&removed)?;
        }
        self.cache.insert(id.to_string(), None);
        self.dirty.insert(id.to_string());
        self.row_count -= 1;
        self.fold(id, &removed.vector, removed.magnitude);
        Ok(true)
    }

    /// Links each orphaned neighbor to its nearest former co-neighbors that
    /// still have room, so a deleted node does not split its neighborhood.
    fn reconnect(&mut self, ring: &[String], layer: usize) -> Outcome<()> {
        for member in ring {
            let query = self.query_of(member)?;
            let linked = self.links(member, layer)?;
            let mut candidates = Vec::new();
            for other in ring {
                if other != member && !linked.contains(other) {
                    candidates.push((self.distance_to(&query, other)?, other.clone()));
                }
            }
            candidates.sort_by(|left, right| {
                left.0
                    .total_cmp(&right.0)
                    .then_with(|| left.1.cmp(&right.1))
            });
            for (_, other) in candidates {
                if self.links(member, layer)?.len() >= self.m {
                    break;
                }
                if self.links(&other, layer)?.len() >= self.m {
                    continue;
                }
                self.push_link(member, &other, layer)?;
                self.push_link(&other, member, layer)?;
            }
        }
        Ok(())
    }

    /// Promotes the tallest neighbor on the deleted entry point's highest
    /// populated layer.
    fn reelect_entry_point(&mut self, removed: &HnswGraphNode) -> Outcome<()> {
        let query = GraphDistanceQuery::from_parts(removed.vector.clone(), removed.magnitude);
        for layer in (0..removed.layers.len()).rev() {
            let mut best: Option<(usize, f64, String)> = None;
            for neighbor in &removed.layers[layer] {
                let height = self.require(neighbor)?.layers.len();
                let distance = self.distance_to(&query, neighbor)?;
                let better = best
                    .as_ref()
                    .is_none_or(|(best_height, best_distance, best_id)| {
                        best_height
                            .cmp(&height)
                            .then_with(|| distance.total_cmp(best_distance))
                            .then_with(|| neighbor.cmp(best_id))
                            .is_lt()
                    });
                if better {
                    best = Some((height, distance, neighbor.clone()));
                }
            }
            if let Some((height, _, id)) = best {
                self.entry_point = Some(id);
                self.max_layer = height.saturating_sub(1);
                return Ok(());
            }
        }
        if self.row_count == 1 {
            self.entry_point = None;
            self.max_layer = 0;
            return Ok(());
        }
        inconsistent("entry-point-reelection")
    }

    fn fold(&mut self, id: &str, values: &[f32], magnitude: f64) {
        self.source_fingerprint = fold_record_fingerprint(
            self.source_fingerprint,
            normalized_vector_record_fingerprint(id, self.metric, magnitude, values),
        );
    }
}
