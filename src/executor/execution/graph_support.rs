use std::collections::VecDeque;

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
        .capacity()
        .saturating_add(path.node_id.capacity())
        .saturating_add(
            path.path_nodes
                .capacity()
                .saturating_mul(std::mem::size_of::<(String, String)>()),
        )
        .saturating_add(
            path.path_nodes
                .iter()
                .map(|(node_type, node_id)| node_type.capacity().saturating_add(node_id.capacity()))
                .sum(),
        )
        .saturating_add(
            path.path_edges
                .capacity()
                .saturating_mul(std::mem::size_of::<String>()),
        )
        .saturating_add(path.path_edges.iter().map(String::capacity).sum())
        .saturating_add(path.last_edge.as_ref().map_or(0, graph_edge_bytes))
        .saturating_add(std::mem::size_of::<GraphPath>())
}

pub(super) fn initial_graph_path_bytes(node_type: &str, node_id: &str) -> usize {
    std::mem::size_of::<GraphPath>()
        .saturating_add(std::mem::size_of::<(String, String)>())
        .saturating_add(node_type.len().saturating_mul(2))
        .saturating_add(node_id.len().saturating_mul(2))
}

pub(super) fn initial_graph_queue(
    controls: &crate::runtime::QueryExecutionControls,
    node_type: &str,
    node_id: &str,
) -> Result<(VecDeque<GraphPath>, crate::runtime::QueryMemoryReservation), QueryError> {
    let mut memory = controls.reserve_query_memory(0)?;
    let estimated_bytes = initial_graph_path_bytes(node_type, node_id);
    memory.try_grow(estimated_bytes)?;
    let mut queue = VecDeque::new();
    try_reserve_graph_slot(|| queue.try_reserve(1))?;
    let path = GraphPath {
        node_type: node_type.to_owned(),
        node_id: node_id.to_owned(),
        depth: 0,
        cost: 0.0,
        path_nodes: vec![(node_type.to_owned(), node_id.to_owned())],
        path_edges: Vec::new(),
        last_edge: None,
    };
    reconcile_graph_path_memory(&mut memory, estimated_bytes, graph_path_bytes(&path))?;
    queue.push_back(path);
    Ok((queue, memory))
}

pub(super) fn initial_graph_frontier(
    controls: &crate::runtime::QueryExecutionControls,
    node_type: String,
    node_id: String,
) -> Result<(Vec<GraphPath>, crate::runtime::QueryMemoryReservation), QueryError> {
    let mut memory = controls.reserve_query_memory(0)?;
    let estimated_bytes = initial_graph_path_bytes(&node_type, &node_id)
        .saturating_sub(std::mem::size_of::<GraphPath>());
    memory.try_grow(estimated_bytes)?;
    let mut frontier = Vec::new();
    try_reserve_graph_path_slot(&mut frontier, &mut memory)?;
    let path = GraphPath {
        node_type: node_type.clone(),
        node_id: node_id.clone(),
        depth: 0,
        cost: 0.0,
        path_nodes: vec![(node_type, node_id)],
        path_edges: Vec::new(),
        last_edge: None,
    };
    reconcile_graph_path_memory(
        &mut memory,
        estimated_bytes,
        graph_path_bytes(&path).saturating_sub(std::mem::size_of::<GraphPath>()),
    )?;
    frontier.push(path);
    Ok((frontier, memory))
}

pub(super) fn neighbor_graph_path_bytes(
    edge: &GraphEdgeRecord,
    start_type: &str,
    start_id: &str,
    next_type: &str,
    next_id: &str,
) -> usize {
    std::mem::size_of::<GraphPath>()
        .saturating_add(2 * std::mem::size_of::<(String, String)>())
        .saturating_add(std::mem::size_of::<String>())
        .saturating_add(start_type.len())
        .saturating_add(start_id.len())
        .saturating_add(next_type.len().saturating_mul(2))
        .saturating_add(next_id.len().saturating_mul(2))
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
        .saturating_add(
            path.path_nodes
                .len()
                .saturating_add(1)
                .saturating_mul(std::mem::size_of::<(String, String)>()),
        )
        .saturating_add(
            path.path_edges
                .len()
                .saturating_add(1)
                .saturating_mul(std::mem::size_of::<String>()),
        )
        .saturating_add(next_type.len().saturating_mul(2))
        .saturating_add(next_id.len().saturating_mul(2))
        .saturating_add(
            path.path_nodes
                .iter()
                .map(|(node_type, node_id)| node_type.capacity().saturating_add(node_id.capacity()))
                .sum::<usize>(),
        )
        .saturating_add(path.path_edges.iter().map(String::capacity).sum::<usize>())
        .saturating_add(edge.edge_id.capacity())
        .saturating_add(graph_edge_bytes(edge))
}

pub(super) fn graph_output_variable_bytes(path_bytes: usize, node_count: usize) -> usize {
    // Every path node materializes a two-entry JSON object. Its BTreeMap leaf has
    // eleven key/value slots even when only two are used; include that allocation
    // and the JSON array slots in addition to the path's identifier buffers.
    path_bytes
        .saturating_mul(3)
        .saturating_add(node_count.saturating_mul(1_024))
        .saturating_add(2_048)
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

pub(super) fn release_graph_path_memory(
    memory: &mut crate::runtime::QueryMemoryReservation,
    path_bytes: usize,
) {
    release_graph_bytes(
        memory,
        path_bytes.saturating_sub(std::mem::size_of::<GraphPath>()),
    );
}

pub(super) fn try_reserve_graph_path_slot(
    paths: &mut Vec<GraphPath>,
    memory: &mut crate::runtime::QueryMemoryReservation,
) -> Result<(), QueryError> {
    if paths.len() < paths.capacity() {
        return Ok(());
    }
    let previous_capacity = paths.capacity();
    let slot_bytes = std::mem::size_of::<GraphPath>();
    memory.try_grow(slot_bytes)?;
    if let Err(error) = try_reserve_graph_slot(|| paths.try_reserve_exact(1)) {
        release_graph_bytes(memory, slot_bytes);
        return Err(error);
    }
    let retained_bytes = paths
        .capacity()
        .saturating_sub(previous_capacity)
        .saturating_mul(slot_bytes);
    reconcile_graph_path_memory(memory, slot_bytes, retained_bytes)
}

pub(super) fn reconcile_graph_path_memory(
    memory: &mut crate::runtime::QueryMemoryReservation,
    estimated_bytes: usize,
    retained_bytes: usize,
) -> Result<(), QueryError> {
    if retained_bytes > estimated_bytes {
        memory.try_grow(retained_bytes.saturating_sub(estimated_bytes))?;
    } else if estimated_bytes > retained_bytes {
        release_graph_bytes(memory, estimated_bytes.saturating_sub(retained_bytes));
    }
    Ok(())
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
        .capacity()
        .saturating_add(edge.graph.capacity())
        .saturating_add(edge.row_id.capacity())
        .saturating_add(edge.edge_type.capacity())
        .saturating_add(edge.source_type.capacity())
        .saturating_add(edge.source_id.capacity())
        .saturating_add(edge.target_type.capacity())
        .saturating_add(edge.target_id.capacity())
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
        .then_with(|| left.row_id.cmp(&right.row_id))
}

pub(super) fn same_executor_graph_edge(left: &GraphEdgeRecord, right: &GraphEdgeRecord) -> bool {
    left.graph_id == right.graph_id
        && left.row_id == right.row_id
        && left.edge_id == right.edge_id
        && left.source_type == right.source_type
        && left.source_id == right.source_id
        && left.target_type == right.target_type
        && left.target_id == right.target_id
        && left.edge_type == right.edge_type
        && left.weight.to_bits() == right.weight.to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_keep_shortest_frontier_slots_budgeted_after_popping_paths() {
        // Arrange
        let controls = crate::runtime::QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        );
        let (mut frontier, mut memory) =
            initial_graph_frontier(&controls, "n".to_string(), "s".to_string())
                .expect("initial frontier");

        // Act
        let path = frontier.pop().expect("initial path");
        release_graph_path_memory(&mut memory, graph_path_bytes(&path));
        drop(path);

        // Assert
        let retained_slots = frontier.capacity() * std::mem::size_of::<GraphPath>();
        assert!(
            memory.bytes() >= retained_slots,
            "popping paths must keep backing storage budgeted: reserved {}, retained {retained_slots}",
            memory.bytes()
        );
        drop(frontier);
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_account_for_retained_graph_path_allocations() {
        // Arrange
        let string = |value: &str, capacity: usize| {
            let mut value_with_capacity = String::with_capacity(capacity);
            value_with_capacity.push_str(value);
            value_with_capacity
        };
        let mut path_nodes = Vec::with_capacity(3);
        path_nodes.push((string("n", 4), string("s", 8)));
        path_nodes.push((string("n", 4), string("t", 8)));
        let mut path_edges = Vec::with_capacity(4);
        path_edges.push(string("e", 8));
        let edge = GraphEdgeRecord {
            graph: string("social", 16),
            graph_id: 1,
            row_id: string("row", 8),
            edge_id: string("edge", 8),
            source_type: string("n", 4),
            source_id: string("s", 8),
            target_type: string("n", 4),
            target_id: string("t", 8),
            edge_type: string("r", 4),
            weight: 1.0,
        };
        let path = GraphPath {
            node_type: string("n", 4),
            node_id: string("t", 8),
            depth: 1,
            cost: 1.0,
            path_nodes,
            path_edges,
            last_edge: Some(edge),
        };

        // Act
        let accounted_bytes = graph_path_bytes(&path);
        let retained_bytes = std::mem::size_of::<GraphPath>()
            + path.node_type.capacity()
            + path.node_id.capacity()
            + path.path_nodes.capacity() * std::mem::size_of::<(String, String)>()
            + path
                .path_nodes
                .iter()
                .map(|(node_type, node_id)| node_type.capacity() + node_id.capacity())
                .sum::<usize>()
            + path.path_edges.capacity() * std::mem::size_of::<String>()
            + path.path_edges.iter().map(String::capacity).sum::<usize>()
            + path.last_edge.as_ref().map_or(0, |edge| {
                edge.graph.capacity()
                    + edge.row_id.capacity()
                    + edge.edge_id.capacity()
                    + edge.source_type.capacity()
                    + edge.source_id.capacity()
                    + edge.target_type.capacity()
                    + edge.target_id.capacity()
                    + edge.edge_type.capacity()
            });

        // Assert
        assert_eq!(
            accounted_bytes, retained_bytes,
            "every retained path allocation must count against the query budget"
        );
    }

    #[test]
    fn should_budget_materialized_path_json_values() {
        // Arrange
        fn json_buffer_bytes(value: &serde_json::Value) -> usize {
            match value {
                serde_json::Value::Null
                | serde_json::Value::Bool(_)
                | serde_json::Value::Number(_) => 0,
                serde_json::Value::String(value) => value.capacity(),
                serde_json::Value::Array(values) => {
                    values.capacity() * std::mem::size_of::<serde_json::Value>()
                        + values.iter().map(json_buffer_bytes).sum::<usize>()
                }
                serde_json::Value::Object(values) => {
                    // Each two-entry node object occupies a BTreeMap leaf with eleven slots.
                    values.len().max(11) * std::mem::size_of::<(String, serde_json::Value)>()
                        + values
                            .iter()
                            .map(|(key, value)| key.capacity() + json_buffer_bytes(value))
                            .sum::<usize>()
                }
            }
        }

        let mut path_nodes = Vec::with_capacity(128);
        let mut path_edges = Vec::with_capacity(127);
        for index in 0..128 {
            path_nodes.push((format!("t{index}"), format!("n{index}")));
            if index > 0 {
                path_edges.push(format!("e{index}"));
            }
        }
        let path = GraphPath {
            node_type: "t".to_string(),
            node_id: "n127".to_string(),
            depth: 127,
            cost: 127.0,
            path_nodes,
            path_edges,
            last_edge: None,
        };

        // Act
        let estimated_bytes =
            graph_output_variable_bytes(graph_path_bytes(&path), path.path_nodes.len());
        let row = path.into_row(1);
        let entries = row.into_entries();
        let minimum_retained_bytes = entries
            .capacity()
            .saturating_mul(std::mem::size_of::<(String, crate::types::Value)>())
            .saturating_add(
                entries
                    .iter()
                    .map(|(name, value)| {
                        let value_bytes = match value {
                            crate::types::Value::String(value) => value.capacity(),
                            crate::types::Value::Json(value) => json_buffer_bytes(value),
                            _ => 0,
                        };
                        name.capacity().saturating_add(value_bytes)
                    })
                    .sum::<usize>(),
            );

        // Assert
        assert!(
            estimated_bytes >= minimum_retained_bytes,
            "path row reservation must cover retained values: reserved {estimated_bytes}, retained at least {minimum_retained_bytes}"
        );
    }
}
