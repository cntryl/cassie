use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use cassie_core::request::{Interruption, InterruptionError, MemoryBudgetError, RequestMemory, RequestMemoryReservation};
use crate::types::{DistanceMetric, HnswGraphNode, HnswIndexOptions};

use super::{
    compare_hnsw_candidates, exact_distance, graph_distance, GraphDistanceQuery, HnswCandidate,
    HnswSearchResult, OrderedCandidateSet, SearchCandidate,
};

#[cfg(test)]
#[path = "controlled/tests.rs"]
mod tests;

#[derive(Debug)]
pub enum HnswSearchError<E> {
    Loader(E),
    GraphRepair(HnswGraphRepairReason),
    Interruption(InterruptionError),
    Memory(MemoryBudgetError),
    Allocation,
    AccountingOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HnswGraphRepairReason {
    MissingEntryPoint,
    InconsistentMaxLayer,
}

impl<E> From<MemoryBudgetError> for HnswSearchError<E> {
    fn from(error: MemoryBudgetError) -> Self { Self::Memory(error) }
}
impl<E> From<InterruptionError> for HnswSearchError<E> {
    fn from(error: InterruptionError) -> Self { Self::Interruption(error) }
}

/// Searches a persisted graph through an unaccounted point-read node loader.
pub fn search_graph_with_node_loader(
    metric: DistanceMetric,
    entry_point: &str,
    max_layer: usize,
    query: &[f32],
    options: &HnswIndexOptions,
    limit: usize,
    mut load_node: impl FnMut(&str) -> Option<HnswGraphNode>,
) -> Option<HnswSearchResult> {
    let distance_query = GraphDistanceQuery::from_query(query)?;
    let mut cache = BTreeMap::<String, Option<HnswGraphNode>>::new();
    let mut load = |id: &str| {
        cache
            .entry(id.to_string())
            .or_insert_with(|| load_node(id))
            .clone()
    };
    let mut current = entry_point.to_string();
    for layer in (1..=max_layer).rev() {
        current = greedy_layer_search_loaded(metric, &distance_query, &current, layer, &mut load);
    }
    let ef_search = options.ef_search.max(limit).max(1);
    let candidates =
        search_graph_layer_loaded(metric, &distance_query, &current, 0, ef_search, &mut load);
    build_search_result(metric, query, limit, candidates, &mut load)
}

pub struct ControlledHnswSearchRequest<'a> {
    pub metric: DistanceMetric,
    pub entry_point: &'a str,
    pub max_layer: usize,
    pub query: &'a [f32],
    pub options: &'a HnswIndexOptions,
    pub limit: usize,
}

struct ControlledLayerContext<'a, E> {
    metric: DistanceMetric,
    query: &'a GraphDistanceQuery,
    layer: usize,
    interruption: Interruption<'a>,
    _error: std::marker::PhantomData<fn() -> E>,
}

struct ControlledCachedNode {
    node: Option<Arc<HnswGraphNode>>,
    _memory: RequestMemoryReservation,
}

pub fn search_graph_with_controlled_node_loader<E>(
    request: &ControlledHnswSearchRequest<'_>,
    interruption: Interruption<'_>,
    request_memory: &RequestMemory,
    mut load_node: impl FnMut(
        &str,
    )
        -> Result<Option<(HnswGraphNode, RequestMemoryReservation)>, E>,
) -> Result<Option<(HnswSearchResult, RequestMemoryReservation)>, HnswSearchError<E>> {
    check_controls(interruption)?;
    let query_bytes = request
        .query
        .len()
        .saturating_mul(2)
        .saturating_mul(std::mem::size_of::<f32>());
    let _query_memory = request_memory.reserve(query_bytes)?;
    let Some(distance_query) = GraphDistanceQuery::from_query(request.query) else {
        return Ok(None);
    };
    let mut traversal_memory = request_memory.reserve(0)?;
    let mut cache = BTreeMap::<String, ControlledCachedNode>::new();
    let mut load = |id: &str| -> Result<Option<Arc<HnswGraphNode>>, HnswSearchError<E>> {
        load_controlled_cached_node(&mut cache, id, interruption, request_memory, &mut load_node)
    };

    traversal_memory.try_grow(request.entry_point.len())?;
    let mut current = request.entry_point.to_owned();
    let entry = load(request.entry_point)?
        .ok_or(HnswSearchError::GraphRepair(
            HnswGraphRepairReason::MissingEntryPoint,
        ))?;
    if entry.layers.len() <= request.max_layer {
        return Err(HnswSearchError::GraphRepair(
            HnswGraphRepairReason::InconsistentMaxLayer,
        ));
    }
    for layer in (1..=request.max_layer).rev() {
        current = greedy_layer_search_loaded_controlled(
            &ControlledLayerContext {
                metric: request.metric,
                query: &distance_query,
                layer,
                interruption,
                _error: std::marker::PhantomData,
            },
            &current,
            &mut traversal_memory,
            &mut load,
        )?;
    }
    let ef_search = request.options.ef_search.max(request.limit).max(1);
    let candidates = search_graph_layer_loaded_controlled(
        &ControlledLayerContext {
            metric: request.metric,
            query: &distance_query,
            layer: 0,
                interruption,
            _error: std::marker::PhantomData,
        },
        &current,
        ef_search,
        &mut traversal_memory,
        &mut load,
    )?;
    let candidate_count = candidates.len();
    let mut exact = Vec::new();
    for candidate in candidates {
        check_controls(interruption)?;
        let Some(node) = load(&candidate.id)? else {
            continue;
        };
        traversal_memory
            .try_grow(std::mem::size_of::<HnswCandidate>().saturating_add(candidate.id.len()))?;
        exact.try_reserve_exact(1).map_err(|error| {
            let _ = error;
            HnswSearchError::Allocation
        })?;
        exact.push(HnswCandidate {
            id: candidate.id,
            distance: exact_distance(
                request.metric,
                request.query,
                &distance_query.normalized,
                &node,
            ),
        });
    }
    exact.sort_by(compare_hnsw_candidates);
    exact.truncate(request.limit.max(1));
    check_controls(interruption)?;
    // Transfer the guard with the candidate buffer so it stays reserved across this boundary.
    Ok(Some((
        HnswSearchResult {
            candidates: exact,
            candidate_count,
        },
        traversal_memory,
    )))
}

fn load_controlled_cached_node<E>(
    cache: &mut BTreeMap<String, ControlledCachedNode>,
    id: &str,
    interruption: Interruption<'_>,
    request_memory: &RequestMemory,
    load_node: &mut impl FnMut(
        &str,
    )
        -> Result<Option<(HnswGraphNode, RequestMemoryReservation)>, E>,
) -> Result<Option<Arc<HnswGraphNode>>, HnswSearchError<E>> {
    check_controls(interruption)?;
    if let Some(cached) = cache.get(id) {
        return Ok(cached.node.clone());
    }
    let (loaded, memory) = match load_node(id).map_err(HnswSearchError::Loader)? {
        Some((node, mut memory)) => {
            let bytes = hnsw_cache_entry_bytes(id, Some(&node))?;
            if bytes > memory.bytes() {
                memory.try_grow(bytes - memory.bytes())?;
            } else {
                memory.shrink_to(bytes);
            }
            (Some(Arc::new(node)), memory)
        }
        None => (
            None,
            request_memory.reserve(hnsw_cache_entry_bytes(id, None)?)?,
        ),
    };
    cache.insert(
        id.to_owned(),
        ControlledCachedNode {
            node: loaded.clone(),
            _memory: memory,
        },
    );
    Ok(loaded)
}

fn build_search_result(
    metric: DistanceMetric,
    query: &[f32],
    limit: usize,
    candidates: Vec<SearchCandidate>,
    load: &mut impl FnMut(&str) -> Option<HnswGraphNode>,
) -> Option<HnswSearchResult> {
    let normalized_query = crate::normalize(query)?;
    let candidate_count = candidates.len();
    let mut exact = candidates
        .into_iter()
        .filter_map(|candidate| {
            let node = load(&candidate.id)?;
            Some(HnswCandidate {
                id: candidate.id,
                distance: exact_distance(metric, query, &normalized_query.values, &node),
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

fn greedy_layer_search_loaded(
    metric: DistanceMetric,
    query: &GraphDistanceQuery,
    entry_point: &str,
    layer: usize,
    load: &mut impl FnMut(&str) -> Option<HnswGraphNode>,
) -> String {
    let mut current = entry_point.to_string();
    loop {
        let Some(current_node) = load(&current) else {
            return current;
        };
        let current_distance = graph_distance(metric, query, &current_node);
        let Some(neighbors) = current_node.layers.get(layer) else {
            return current;
        };
        let best = neighbors
            .iter()
            .filter_map(|neighbor| {
                let node = load(neighbor)?;
                Some((neighbor, graph_distance(metric, query, &node)))
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

fn search_graph_layer_loaded(
    metric: DistanceMetric,
    query: &GraphDistanceQuery,
    entry_point: &str,
    layer: usize,
    ef: usize,
    load: &mut impl FnMut(&str) -> Option<HnswGraphNode>,
) -> Vec<SearchCandidate> {
    let Some(entry) = load(entry_point) else {
        return Vec::new();
    };
    let mut visited = BTreeSet::new();
    let entry_candidate = SearchCandidate {
        id: entry.id.clone(),
        distance: graph_distance(metric, query, &entry),
    };
    visited.insert(entry.id.clone());
    let mut candidates = OrderedCandidateSet::unbounded();
    candidates.insert(entry_candidate.clone());
    let mut nearest = OrderedCandidateSet::bounded(ef);
    nearest.insert(entry_candidate);

    while let Some(candidate) = candidates.pop_nearest() {
        let worst_distance = nearest.worst_distance();
        if nearest.len() >= ef && candidate.distance > worst_distance {
            break;
        }
        let Some(node) = load(&candidate.id) else {
            continue;
        };
        let Some(neighbors) = node.layers.get(layer) else {
            continue;
        };
        for neighbor_id in neighbors {
            if !visited.insert(neighbor_id.clone()) {
                continue;
            }
            let Some(neighbor) = load(neighbor_id) else {
                continue;
            };
            let distance = graph_distance(metric, query, &neighbor);
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

fn greedy_layer_search_loaded_controlled<E>(
    context: &ControlledLayerContext<'_, E>,
    entry_point: &str,
    memory: &mut RequestMemoryReservation,
    load: &mut impl FnMut(&str) -> Result<Option<Arc<HnswGraphNode>>, HnswSearchError<E>>,
) -> Result<String, HnswSearchError<E>> {
    memory.try_grow(entry_point.len())?;
    let mut current = entry_point.to_string();
    loop {
        check_controls(context.interruption)?;
        let Some(current_node) = load(&current)? else {
            return Ok(current);
        };
        let current_distance = graph_distance(context.metric, context.query, &current_node);
        let Some(neighbors) = current_node.layers.get(context.layer) else {
            return Ok(current);
        };
        let mut best: Option<(String, f64)> = None;
        for neighbor in neighbors {
            check_controls(context.interruption)?;
            let Some(node) = load(neighbor)? else {
                continue;
            };
            let distance = graph_distance(context.metric, context.query, &node);
            if best.as_ref().is_none_or(|(best_id, best_distance)| {
                distance
                    .total_cmp(best_distance)
                    .then_with(|| neighbor.cmp(best_id))
                    == std::cmp::Ordering::Less
            }) {
                memory.try_grow(neighbor.len())?;
                best = Some((neighbor.clone(), distance));
            }
        }
        let Some((best_id, best_distance)) = best else {
            return Ok(current);
        };
        if best_distance >= current_distance {
            return Ok(current);
        }
        current = best_id;
    }
}

fn search_graph_layer_loaded_controlled<E>(
    context: &ControlledLayerContext<'_, E>,
    entry_point: &str,
    ef: usize,
    memory: &mut RequestMemoryReservation,
    load: &mut impl FnMut(&str) -> Result<Option<Arc<HnswGraphNode>>, HnswSearchError<E>>,
) -> Result<Vec<SearchCandidate>, HnswSearchError<E>> {
    let Some(entry) = load(entry_point)? else {
        return Ok(Vec::new());
    };
    let mut visited = BTreeSet::new();
    reserve_visited_identity(memory, &entry.id)?;
    visited.insert(entry.id.clone());
    let mut candidates = OrderedCandidateSet::unbounded();
    let mut nearest = OrderedCandidateSet::bounded(ef);
    let entry_distance = graph_distance(context.metric, context.query, &entry);
    insert_search_candidate(&mut candidates, memory, &entry.id, entry_distance)?;
    insert_search_candidate(&mut nearest, memory, &entry.id, entry_distance)?;

    while let Some(candidate) = candidates.pop_nearest() {
        check_controls(context.interruption)?;
        let worst_distance = nearest.worst_distance();
        if nearest.len() >= ef && candidate.distance > worst_distance {
            break;
        }
        let Some(node) = load(&candidate.id)? else {
            continue;
        };
        let Some(neighbors) = node.layers.get(context.layer) else {
            continue;
        };
        for neighbor_id in neighbors {
            check_controls(context.interruption)?;
            if visited.contains(neighbor_id) {
                continue;
            }
            reserve_visited_identity(memory, neighbor_id)?;
            visited.insert(neighbor_id.clone());
            let Some(neighbor) = load(neighbor_id)? else {
                continue;
            };
            let distance = graph_distance(context.metric, context.query, &neighbor);
            if nearest.len() < ef || distance < worst_distance {
                insert_search_candidate(&mut candidates, memory, &neighbor.id, distance)?;
                insert_search_candidate(&mut nearest, memory, &neighbor.id, distance)?;
            }
        }
    }
    Ok(nearest.into_vec())
}

fn insert_search_candidate<E>(
    candidates: &mut OrderedCandidateSet,
    memory: &mut RequestMemoryReservation,
    id: &str,
    distance: f64,
) -> Result<(), HnswSearchError<E>> {
    let previous_bytes = memory.bytes();
    memory.try_grow(checked_add_bytes(
        std::mem::size_of::<SearchCandidate>(),
        id.len(),
    )?)?;
    if candidates.entries.try_reserve_exact(1).is_err() {
        memory.shrink_to(previous_bytes);
        return Err(HnswSearchError::Allocation);
    }
    candidates.insert(SearchCandidate {
        id: id.to_owned(),
        distance,
    });
    Ok(())
}

fn reserve_visited_identity<E>(
    memory: &mut RequestMemoryReservation,
    id: &str,
) -> Result<(), HnswSearchError<E>> {
    memory.try_grow(checked_add_bytes(
        btree_node_bytes::<String, (), E>()?,
        id.len(),
    )?)?;
    Ok(())
}

fn hnsw_cache_entry_bytes<E>(id: &str, node: Option<&HnswGraphNode>) -> Result<usize, HnswSearchError<E>> {
    let cache_bytes = checked_add_bytes(
        btree_node_bytes::<String, ControlledCachedNode, E>()?,
        id.len(),
    )?;
    let Some(node) = node else {
        return Ok(cache_bytes);
    };
    let inline = checked_add_bytes(
        std::mem::size_of::<HnswGraphNode>(),
        2 * std::mem::size_of::<usize>(),
    )?;
    let vectors = checked_mul_bytes(node.vector.capacity(), std::mem::size_of::<f32>())?;
    let layers = checked_mul_bytes(node.layers.capacity(), std::mem::size_of::<Vec<String>>())?;
    let neighbors = node.layers.iter().try_fold(layers, |bytes, layer| {
        let slots = checked_mul_bytes(layer.capacity(), std::mem::size_of::<String>())?;
        layer
            .iter()
            .try_fold(checked_add_bytes(bytes, slots)?, |bytes, neighbor| {
                checked_add_bytes(bytes, neighbor.capacity())
            })
    })?;
    checked_add_bytes(
        cache_bytes,
        checked_add_bytes(
            inline,
            checked_add_bytes(node.id.capacity(), checked_add_bytes(vectors, neighbors)?)?,
        )?,
    )
}

fn btree_node_bytes<K, V, E>() -> Result<usize, HnswSearchError<E>> {
    // Charge a complete eleven-slot internal node per entry, including sparse leaf slack.
    checked_add_bytes(
        checked_mul_bytes(
            11,
            checked_add_bytes(std::mem::size_of::<K>(), std::mem::size_of::<V>())?,
        )?,
        16 * std::mem::size_of::<usize>(),
    )
}

fn checked_add_bytes<E>(left: usize, right: usize) -> Result<usize, HnswSearchError<E>> {
    left.checked_add(right).ok_or_else(accounting_overflow)
}

fn checked_mul_bytes<E>(left: usize, right: usize) -> Result<usize, HnswSearchError<E>> {
    left.checked_mul(right).ok_or_else(accounting_overflow)
}

fn accounting_overflow<E>() -> HnswSearchError<E> {
    HnswSearchError::AccountingOverflow
}

fn check_controls<E>(interruption: Interruption<'_>) -> Result<(), HnswSearchError<E>> {
    interruption.check().map_err(HnswSearchError::Interruption)
}
