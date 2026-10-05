use std::cell::Cell;
use std::mem::size_of;
use std::sync::Arc;
use std::time::Instant;

use crate::config::CassieRuntimeLimits;

use super::{
    hnsw_cache_entry_bytes, search_graph_layer_loaded_controlled,
    search_graph_with_controlled_node_loader, ControlledHnswSearchRequest, ControlledLayerContext,
    DistanceMetric, GraphDistanceQuery, HnswCandidate, HnswGraphNode, HnswIndexOptions,
    QueryExecutionControls, SearchCandidate,
};

#[test]
fn should_include_complete_owned_node_storage_in_hnsw_memory() {
    // Arrange
    let mut id = String::with_capacity(16);
    id.push_str("entry");
    let mut vector = Vec::with_capacity(8);
    vector.extend([1.0, 0.0, 0.0]);
    let mut neighbor = String::with_capacity(32);
    neighbor.push_str("neighbor");
    let mut layer = Vec::with_capacity(8);
    layer.push(neighbor);
    let mut layers = Vec::with_capacity(4);
    layers.push(layer);
    let node = HnswGraphNode {
        id,
        vector,
        magnitude: 1.0,
        layers,
    };
    // A populated BTree leaf owns eleven key/value slots even with one cached node.
    let cache_slots = 11 * (size_of::<String>() + size_of::<Option<Arc<HnswGraphNode>>>());
    let retained_node = size_of::<HnswGraphNode>()
        + 2 * size_of::<usize>()
        + node.id.capacity()
        + node.vector.capacity() * size_of::<f32>()
        + node.layers.capacity() * size_of::<Vec<String>>()
        + node
            .layers
            .iter()
            .map(|layer| {
                layer.capacity() * size_of::<String>()
                    + layer.iter().map(String::capacity).sum::<usize>()
            })
            .sum::<usize>();
    let minimum_retained = cache_slots + "entry".len() + retained_node;

    // Act
    let estimate = hnsw_cache_entry_bytes("entry", Some(&node)).expect("cache estimate");

    // Assert
    assert!(
        estimate >= minimum_retained,
        "cache owns at least {minimum_retained} bytes, but reserves {estimate}"
    );
}

#[test]
fn should_reserve_sparse_hnsw_visited_storage_before_loading_a_neighbor() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut memory = controls.reserve_query_memory(0).expect("traversal guard");
    let query = GraphDistanceQuery::from_query(&[1.0, 0.0, 0.0]).expect("query");
    let entry = Arc::new(HnswGraphNode {
        id: "entry".to_owned(),
        vector: vec![1.0, 0.0, 0.0],
        magnitude: 1.0,
        layers: vec![vec!["neighbor".to_owned()]],
    });
    let neighbor = Arc::new(HnswGraphNode {
        id: "neighbor".to_owned(),
        vector: vec![1.0, 0.0, 0.0],
        magnitude: 1.0,
        layers: vec![Vec::new()],
    });
    let observed_neighbor_bytes = Cell::new(0);

    // Act
    let candidates = search_graph_layer_loaded_controlled(
        &ControlledLayerContext {
            metric: DistanceMetric::L2,
            query: &query,
            layer: 0,
            controls: &controls,
        },
        "entry",
        2,
        &mut memory,
        &mut |id| {
            if id == "neighbor" {
                observed_neighbor_bytes.set(controls.current_query_memory_bytes());
                Ok(Some(neighbor.clone()))
            } else {
                Ok(Some(entry.clone()))
            }
        },
    )
    .expect("loaded layer");
    let returned_count = candidates.len();
    drop(candidates);
    drop(memory);

    // Assert
    assert_eq!(returned_count, 2);
    // Fixture Arcs are shared inputs; this lower bound covers only query-owned visited keys.
    let minimum_visited_slots = 11 * size_of::<String>();
    assert!(
        observed_neighbor_bytes.get() >= minimum_visited_slots,
        "visited BTree slots retain at least {minimum_visited_slots} bytes before neighbor load, but reserve {}",
        observed_neighbor_bytes.get()
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_loaded_hnsw_layer_candidate_capacity_reserved() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut memory = controls.reserve_query_memory(0).expect("traversal guard");
    let query = GraphDistanceQuery::from_query(&[1.0, 0.0, 0.0]).expect("query");
    let entry = Arc::new(HnswGraphNode {
        id: "entry".to_owned(),
        vector: vec![1.0, 0.0, 0.0],
        magnitude: 1.0,
        layers: vec![Vec::new()],
    });

    // Act
    let candidates = search_graph_layer_loaded_controlled(
        &ControlledLayerContext {
            metric: DistanceMetric::L2,
            query: &query,
            layer: 0,
            controls: &controls,
        },
        "entry",
        1,
        &mut memory,
        &mut |_| Ok(Some(entry.clone())),
    )
    .expect("loaded layer");
    let retained_bytes = candidates.capacity() * size_of::<SearchCandidate>()
        + candidates
            .iter()
            .map(|candidate| candidate.id.capacity())
            .sum::<usize>();
    let live_bytes = controls.current_query_memory_bytes();
    drop(candidates);
    drop(memory);

    // Assert
    assert!(
        live_bytes >= retained_bytes,
        "candidate Vec retains {retained_bytes} bytes, but only {live_bytes} bytes remain reserved"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_returned_hnsw_candidate_storage_reserved_until_the_result_is_dropped() {
    // Arrange
    let ids = (0..40)
        .map(|index| format!("row-{index:02}"))
        .collect::<Vec<_>>();
    let nodes = ids
        .iter()
        .map(|id| HnswGraphNode {
            id: id.clone(),
            vector: vec![1.0, 0.0, 0.0],
            magnitude: 1.0,
            layers: vec![ids
                .iter()
                .filter(|neighbor| *neighbor != id)
                .cloned()
                .collect()],
        })
        .collect::<Vec<_>>();
    let options = HnswIndexOptions::default();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let request = ControlledHnswSearchRequest {
        metric: DistanceMetric::L2,
        entry_point: &ids[0],
        max_layer: 0,
        query: &[1.0, 0.0, 0.0],
        options: &options,
        limit: 1,
    };

    // Act
    let result = search_graph_with_controlled_node_loader(&request, &controls, |id| {
        let Some(node) = nodes.iter().find(|node| node.id == id) else {
            return Ok(None);
        };
        let memory = controls.reserve_query_memory(hnsw_cache_entry_bytes(id, Some(node))?)?;
        Ok(Some((node.clone(), memory)))
    })
    .expect("controlled HNSW search")
    .expect("candidate result");
    let candidate_count = result.0.candidate_count;
    let returned_count = result.0.candidates.len();
    let retained_bytes = result.0.candidates.capacity() * size_of::<HnswCandidate>()
        + result
            .0
            .candidates
            .iter()
            .map(|candidate| candidate.id.len())
            .sum::<usize>();
    let live_bytes = controls.current_query_memory_bytes();
    drop(result);
    let remaining_bytes = controls.current_query_memory_bytes();

    // Assert
    assert_eq!(candidate_count, options.ef_search);
    assert_eq!(returned_count, 1);
    assert!(
        live_bytes >= retained_bytes,
        "returned candidates retain {retained_bytes} bytes, but only {live_bytes} bytes remain reserved"
    );
    assert_eq!(remaining_bytes, 0);
}
