use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::app::CassieError;
use crate::embeddings::{DistanceMetric, HnswGraphNode, HnswIndexOptions};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{
    compare_hnsw_candidates, exact_distance, graph_distance, GraphDistanceQuery, HnswCandidate,
    HnswSearchResult, OrderedCandidateSet, SearchCandidate,
};

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

pub(crate) struct ControlledHnswSearchRequest<'a> {
    pub(crate) metric: DistanceMetric,
    pub(crate) entry_point: &'a str,
    pub(crate) max_layer: usize,
    pub(crate) query: &'a [f32],
    pub(crate) options: &'a HnswIndexOptions,
    pub(crate) limit: usize,
}

struct ControlledLayerContext<'a> {
    metric: DistanceMetric,
    query: &'a GraphDistanceQuery,
    layer: usize,
    controls: &'a QueryExecutionControls,
}

pub(crate) fn search_graph_with_controlled_node_loader(
    request: &ControlledHnswSearchRequest<'_>,
    controls: &QueryExecutionControls,
    mut load_node: impl FnMut(&str) -> Result<Option<HnswGraphNode>, CassieError>,
) -> Result<Option<HnswSearchResult>, CassieError> {
    check_controls(controls)?;
    let query_bytes = request
        .query
        .len()
        .saturating_mul(2)
        .saturating_mul(std::mem::size_of::<f32>());
    let _query_memory = controls.reserve_query_memory(query_bytes)?;
    let Some(distance_query) = GraphDistanceQuery::from_query(request.query) else {
        return Ok(None);
    };
    let mut cache_memory = controls.reserve_query_memory(0)?;
    let mut traversal_memory = controls.reserve_query_memory(0)?;
    let mut cache = BTreeMap::<String, Option<Arc<HnswGraphNode>>>::new();
    let mut load = |id: &str| -> Result<Option<Arc<HnswGraphNode>>, CassieError> {
        check_controls(controls)?;
        if let Some(node) = cache.get(id) {
            return Ok(node.clone());
        }
        let loaded = load_node(id)?.map(Arc::new);
        cache_memory.try_grow(hnsw_cache_entry_bytes(id, loaded.as_deref()))?;
        cache.insert(id.to_string(), loaded.clone());
        Ok(loaded)
    };

    let mut current = request.entry_point.to_string();
    traversal_memory.try_grow(current.len())?;
    let entry = load(request.entry_point)?
        .ok_or_else(|| CassieError::Execution("hnsw fallback:missing-entry-point".to_string()))?;
    if entry.layers.len() <= request.max_layer {
        return Err(CassieError::Execution(
            "hnsw fallback:inconsistent-max-layer".to_string(),
        ));
    }
    for layer in (1..=request.max_layer).rev() {
        current = greedy_layer_search_loaded_controlled(
            &ControlledLayerContext {
                metric: request.metric,
                query: &distance_query,
                layer,
                controls,
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
            controls,
        },
        &current,
        ef_search,
        &mut traversal_memory,
        &mut load,
    )?;
    let candidate_count = candidates.len();
    let mut exact = Vec::new();
    for candidate in candidates {
        check_controls(controls)?;
        let Some(node) = load(&candidate.id)? else {
            continue;
        };
        traversal_memory
            .try_grow(std::mem::size_of::<HnswCandidate>().saturating_add(candidate.id.len()))?;
        exact.try_reserve_exact(1).map_err(|error| {
            CassieError::ResourceLimit(format!(
                "unable to retain controlled HNSW candidate: {error}"
            ))
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
    check_controls(controls)?;
    Ok(Some(HnswSearchResult {
        candidates: exact,
        candidate_count,
    }))
}

fn build_search_result(
    metric: DistanceMetric,
    query: &[f32],
    limit: usize,
    candidates: Vec<SearchCandidate>,
    load: &mut impl FnMut(&str) -> Option<HnswGraphNode>,
) -> Option<HnswSearchResult> {
    let normalized_query = crate::vector::normalize(query)?;
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

fn greedy_layer_search_loaded_controlled(
    context: &ControlledLayerContext<'_>,
    entry_point: &str,
    memory: &mut QueryMemoryReservation,
    load: &mut impl FnMut(&str) -> Result<Option<Arc<HnswGraphNode>>, CassieError>,
) -> Result<String, CassieError> {
    let mut current = entry_point.to_string();
    loop {
        check_controls(context.controls)?;
        let Some(current_node) = load(&current)? else {
            return Ok(current);
        };
        let current_distance = graph_distance(context.metric, context.query, &current_node);
        let Some(neighbors) = current_node.layers.get(context.layer) else {
            return Ok(current);
        };
        let mut best: Option<(String, f64)> = None;
        for neighbor in neighbors {
            check_controls(context.controls)?;
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

fn search_graph_layer_loaded_controlled(
    context: &ControlledLayerContext<'_>,
    entry_point: &str,
    ef: usize,
    memory: &mut QueryMemoryReservation,
    load: &mut impl FnMut(&str) -> Result<Option<Arc<HnswGraphNode>>, CassieError>,
) -> Result<Vec<SearchCandidate>, CassieError> {
    let Some(entry) = load(entry_point)? else {
        return Ok(Vec::new());
    };
    let mut visited = BTreeSet::new();
    let entry_candidate = SearchCandidate {
        id: entry.id.clone(),
        distance: graph_distance(context.metric, context.query, &entry),
    };
    reserve_search_candidate(memory, &entry_candidate, 3)?;
    visited.insert(entry.id.clone());
    let mut candidates = OrderedCandidateSet::unbounded();
    candidates.insert(entry_candidate.clone());
    let mut nearest = OrderedCandidateSet::bounded(ef);
    nearest.insert(entry_candidate);

    while let Some(candidate) = candidates.pop_nearest() {
        check_controls(context.controls)?;
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
            check_controls(context.controls)?;
            if visited.contains(neighbor_id) {
                continue;
            }
            memory.try_grow(std::mem::size_of::<String>().saturating_add(neighbor_id.len()))?;
            visited.insert(neighbor_id.clone());
            let Some(neighbor) = load(neighbor_id)? else {
                continue;
            };
            let distance = graph_distance(context.metric, context.query, &neighbor);
            if nearest.len() < ef || distance < worst_distance {
                let next = SearchCandidate {
                    id: neighbor.id.clone(),
                    distance,
                };
                reserve_search_candidate(memory, &next, 2)?;
                candidates.insert(next.clone());
                nearest.insert(next);
            }
        }
    }
    Ok(nearest.into_vec())
}

fn reserve_search_candidate(
    memory: &mut QueryMemoryReservation,
    candidate: &SearchCandidate,
    copies: usize,
) -> Result<(), CassieError> {
    memory.try_grow(
        std::mem::size_of::<SearchCandidate>()
            .saturating_add(candidate.id.len())
            .saturating_mul(copies),
    )
}

fn hnsw_cache_entry_bytes(id: &str, node: Option<&HnswGraphNode>) -> usize {
    let node_bytes = node.map_or(0, |node| {
        std::mem::size_of::<HnswGraphNode>()
            .saturating_add(node.id.len())
            .saturating_add(node.vector.len().saturating_mul(std::mem::size_of::<f32>()))
            .saturating_add(
                node.layers
                    .iter()
                    .flatten()
                    .map(|neighbor| std::mem::size_of::<String>().saturating_add(neighbor.len()))
                    .sum::<usize>(),
            )
    });
    std::mem::size_of::<String>()
        .saturating_add(id.len())
        .saturating_add(node_bytes)
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}
