use std::collections::{HashSet, VecDeque};

use super::{source, BatchRow, QueryError};
use crate::midge::adapter::GraphEdgeRecord;

#[derive(Debug, Clone)]
pub(super) struct GraphPath {
    pub(super) node_type: String,
    pub(super) node_id: String,
    pub(super) depth: i64,
    pub(super) cost: f64,
    pub(super) path_nodes: Vec<(String, String)>,
    pub(super) path_edges: Vec<String>,
    pub(super) last_edge: Option<crate::midge::adapter::GraphEdgeRecord>,
}

#[derive(Default)]
pub(super) struct GraphExecutionEvidence {
    pub(super) reads: usize,
    pub(super) candidates: usize,
    pub(super) fallback_reason: Option<&'static str>,
}

pub(super) struct LoadedGraphEdges {
    pub(super) values: Vec<GraphEdgeRecord>,
    pub(super) _memory: crate::runtime::QueryMemoryReservation,
}

pub(super) struct GraphEdgeRequest<'a> {
    pub(super) graph: &'a crate::catalog::GraphMeta,
    pub(super) node_type: &'a str,
    pub(super) node_id: &'a str,
    pub(super) direction: &'a str,
    pub(super) edge_types: &'a [String],
    pub(super) limit: Option<usize>,
}

pub(in crate::executor::execution) struct GraphTableRows {
    pub(super) rows: Vec<BatchRow>,
    pub(super) memory: crate::runtime::QueryMemoryReservation,
}

impl GraphTableRows {
    pub(in crate::executor::execution) fn into_parts(
        self,
    ) -> (Vec<BatchRow>, crate::runtime::QueryMemoryReservation) {
        (self.rows, self.memory)
    }
}

impl GraphExecutionEvidence {
    pub(super) fn publish(&self, env: &source::SourceExecutionEnv<'_>) {
        env.cassie
            .runtime
            .record_graph_read_evidence(self.reads, self.candidates);
        env.cassie
            .runtime
            .record_graph_fallback(self.fallback_reason.unwrap_or_default());
    }
}

pub(super) fn graph_path_bytes(path: &GraphPath) -> usize {
    path.node_type
        .len()
        .saturating_add(path.node_id.len())
        .saturating_add(
            path.path_nodes
                .iter()
                .map(|(node_type, node_id)| node_type.len().saturating_add(node_id.len()))
                .sum(),
        )
        .saturating_add(path.path_edges.iter().map(String::len).sum())
        .saturating_add(path.last_edge.as_ref().map_or(0, graph_edge_bytes))
        .saturating_add(std::mem::size_of::<GraphPath>())
}

pub(super) fn initial_graph_path_bytes(node_type: &str, node_id: &str) -> usize {
    std::mem::size_of::<GraphPath>()
        .saturating_add(node_type.len().saturating_mul(2))
        .saturating_add(node_id.len().saturating_mul(2))
}

pub(super) fn initial_graph_queue(
    controls: &crate::runtime::QueryExecutionControls,
    node_type: &str,
    node_id: &str,
) -> Result<(VecDeque<GraphPath>, crate::runtime::QueryMemoryReservation), QueryError> {
    let mut memory = controls.reserve_query_memory(0)?;
    memory.try_grow(initial_graph_path_bytes(node_type, node_id))?;
    let mut queue = VecDeque::new();
    try_reserve_graph_slot(|| queue.try_reserve(1))?;
    queue.push_back(GraphPath {
        node_type: node_type.to_owned(),
        node_id: node_id.to_owned(),
        depth: 0,
        cost: 0.0,
        path_nodes: vec![(node_type.to_owned(), node_id.to_owned())],
        path_edges: Vec::new(),
        last_edge: None,
    });
    Ok((queue, memory))
}

pub(super) fn initial_graph_frontier(
    controls: &crate::runtime::QueryExecutionControls,
    node_type: String,
    node_id: String,
) -> Result<(Vec<GraphPath>, crate::runtime::QueryMemoryReservation), QueryError> {
    let mut memory = controls.reserve_query_memory(0)?;
    memory.try_grow(initial_graph_path_bytes(&node_type, &node_id))?;
    let mut frontier = Vec::new();
    try_reserve_graph_slot(|| frontier.try_reserve(1))?;
    frontier.push(GraphPath {
        node_type: node_type.clone(),
        node_id: node_id.clone(),
        depth: 0,
        cost: 0.0,
        path_nodes: vec![(node_type, node_id)],
        path_edges: Vec::new(),
        last_edge: None,
    });
    Ok((frontier, memory))
}

pub(super) fn record_shortest_visit(
    path: &GraphPath,
    target_type: &str,
    target_id: &str,
    best_seen: &mut HashSet<(String, String)>,
    best_seen_bytes: &mut usize,
    state_memory: &mut crate::runtime::QueryMemoryReservation,
) -> Result<(bool, bool), QueryError> {
    let is_target = path.node_type == target_type && path.node_id == target_id;
    let visited_bytes = graph_node_key_bytes(&path.node_type, &path.node_id);
    state_memory.try_grow(visited_bytes)?;
    let visited_key = (path.node_type.clone(), path.node_id.clone());
    let inserted = if best_seen.contains(&visited_key) {
        drop(visited_key);
        release_graph_bytes(state_memory, visited_bytes);
        false
    } else {
        try_reserve_graph_slot(|| best_seen.try_reserve(1))?;
        best_seen.insert(visited_key)
    };
    if inserted {
        *best_seen_bytes = best_seen_bytes.saturating_add(visited_bytes);
    }
    Ok((inserted, is_target))
}

pub(super) fn neighbor_graph_path_bytes(
    edge: &GraphEdgeRecord,
    start_type: &str,
    start_id: &str,
    next_type: &str,
    next_id: &str,
) -> usize {
    std::mem::size_of::<GraphPath>()
        .saturating_add(start_type.len())
        .saturating_add(start_id.len())
        .saturating_add(next_type.len())
        .saturating_add(next_id.len())
        .saturating_add(edge.edge_id.len())
        .saturating_add(graph_edge_bytes(edge))
}

pub(super) fn next_graph_path_bytes(
    path: &GraphPath,
    edge: &GraphEdgeRecord,
    next_type: &str,
    next_id: &str,
) -> usize {
    std::mem::size_of::<GraphPath>()
        .saturating_add(next_type.len().saturating_mul(2))
        .saturating_add(next_id.len().saturating_mul(2))
        .saturating_add(
            path.path_nodes
                .iter()
                .map(|(node_type, node_id)| node_type.len().saturating_add(node_id.len()))
                .sum::<usize>(),
        )
        .saturating_add(path.path_edges.iter().map(String::len).sum::<usize>())
        .saturating_add(edge.edge_id.len())
        .saturating_add(graph_edge_bytes(edge))
}

pub(super) fn graph_node_key_bytes(node_type: &str, node_id: &str) -> usize {
    std::mem::size_of::<(String, String)>()
        .saturating_add(node_type.len())
        .saturating_add(node_id.len())
        .saturating_add(std::mem::size_of::<usize>().saturating_mul(2))
}

pub(super) fn graph_output_variable_bytes(path_bytes: usize) -> usize {
    path_bytes.saturating_mul(2).saturating_add(512)
}

pub(super) fn release_graph_bytes(
    memory: &mut crate::runtime::QueryMemoryReservation,
    released_bytes: usize,
) {
    let retained_bytes = memory
        .bytes()
        .checked_sub(released_bytes)
        .expect("graph state reservation covers every retained value");
    memory.shrink_to(retained_bytes);
}

pub(super) fn try_reserve_graph_slot(
    reserve: impl FnOnce() -> Result<(), std::collections::TryReserveError>,
) -> Result<(), QueryError> {
    reserve().map_err(|error| {
        crate::app::CassieError::ResourceLimit(format!(
            "unable to retain controlled graph state: {error}"
        ))
        .into()
    })
}

pub(super) fn graph_edge_bytes(edge: &crate::midge::adapter::GraphEdgeRecord) -> usize {
    edge.edge_id
        .len()
        .saturating_add(edge.edge_type.len())
        .saturating_add(edge.source_type.len())
        .saturating_add(edge.source_id.len())
        .saturating_add(edge.target_type.len())
        .saturating_add(edge.target_id.len())
        .saturating_add(std::mem::size_of_val(&edge.weight))
}

pub(super) fn graph_edge_document_bytes(
    id: &str,
    payload: &serde_json::Value,
    graph: &crate::catalog::GraphMeta,
) -> usize {
    std::mem::size_of::<GraphEdgeRecord>()
        .saturating_add(id.len())
        .saturating_add(graph.name.len())
        .saturating_add(json_retained_bytes(payload))
}

pub(super) fn json_retained_bytes(value: &serde_json::Value) -> usize {
    let inline = std::mem::size_of::<serde_json::Value>();
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            inline
        }
        serde_json::Value::String(value) => inline.saturating_add(value.len()),
        serde_json::Value::Array(values) => values.iter().fold(inline, |bytes, value| {
            bytes.saturating_add(json_retained_bytes(value))
        }),
        serde_json::Value::Object(values) => values.iter().fold(inline, |bytes, (key, value)| {
            bytes
                .saturating_add(std::mem::size_of::<String>())
                .saturating_add(key.len())
                .saturating_add(json_retained_bytes(value))
        }),
    }
}

pub(super) fn compare_graph_edge_records(
    left: &GraphEdgeRecord,
    right: &GraphEdgeRecord,
) -> std::cmp::Ordering {
    left.weight
        .total_cmp(&right.weight)
        .then_with(|| left.edge_id.cmp(&right.edge_id))
        .then_with(|| left.source_type.cmp(&right.source_type))
        .then_with(|| left.source_id.cmp(&right.source_id))
        .then_with(|| left.target_type.cmp(&right.target_type))
        .then_with(|| left.target_id.cmp(&right.target_id))
}

pub(super) fn same_executor_graph_edge(left: &GraphEdgeRecord, right: &GraphEdgeRecord) -> bool {
    left.graph_id == right.graph_id
        && left.edge_id == right.edge_id
        && left.source_type == right.source_type
        && left.source_id == right.source_id
        && left.target_type == right.target_type
        && left.target_id == right.target_id
        && left.edge_type == right.edge_type
        && left.weight.to_bits() == right.weight.to_bits()
}
