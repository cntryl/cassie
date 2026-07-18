use std::collections::{HashSet, VecDeque};

use super::{check_timeout, filter, source, BatchRow, FunctionCall, QueryError, Value};
use crate::midge::adapter::{
    GraphEdgeRecord, GraphEdgeScanOutcome, GraphEdgeScanRequest, RowDecode,
};
use crate::runtime::accounted::AccountedVec;

#[derive(Debug, Clone)]
struct GraphPath {
    node_type: String,
    node_id: String,
    depth: i64,
    cost: f64,
    path_nodes: Vec<(String, String)>,
    path_edges: Vec<String>,
    last_edge: Option<crate::midge::adapter::GraphEdgeRecord>,
}

#[derive(Default)]
struct GraphExecutionEvidence {
    reads: usize,
    candidates: usize,
    fallback_reason: Option<&'static str>,
}

struct LoadedGraphEdges {
    values: Vec<GraphEdgeRecord>,
    _memory: crate::runtime::QueryMemoryReservation,
}

struct GraphEdgeRequest<'a> {
    graph: &'a crate::catalog::GraphMeta,
    node_type: &'a str,
    node_id: &'a str,
    direction: &'a str,
    edge_types: &'a [String],
    limit: Option<usize>,
}

pub(super) struct GraphTableRows {
    rows: Vec<BatchRow>,
    memory: crate::runtime::QueryMemoryReservation,
}

impl GraphTableRows {
    pub(super) fn into_parts(self) -> (Vec<BatchRow>, crate::runtime::QueryMemoryReservation) {
        (self.rows, self.memory)
    }
}

impl GraphExecutionEvidence {
    fn publish(&self, env: &source::SourceExecutionEnv<'_>) {
        env.cassie
            .runtime
            .record_graph_read_evidence(self.reads, self.candidates);
        env.cassie
            .runtime
            .record_graph_fallback(self.fallback_reason.unwrap_or_default());
    }
}

pub(super) fn execute_table_function(
    env: &source::SourceExecutionEnv<'_>,
    function: &FunctionCall,
    outer_row: Option<&BatchRow>,
) -> Result<GraphTableRows, QueryError> {
    let args = evaluate_args(env, function, outer_row)?;
    match function.name.to_ascii_lowercase().as_str() {
        "graph_neighbors" => graph_neighbors(env, &args),
        "graph_expand" => graph_expand(env, &args),
        "graph_shortest_path" => graph_shortest_path(env, &args),
        other => Err(QueryError::General(format!(
            "unsupported graph table function '{other}'"
        ))),
    }
}

fn graph_neighbors(
    env: &source::SourceExecutionEnv<'_>,
    args: &[Value],
) -> Result<GraphTableRows, QueryError> {
    let graph_name = text_arg(args, 0, "graph")?;
    let graph = graph_meta(env, &graph_name)?;
    let node_type = text_arg(args, 1, "node_type")?;
    let node_id = text_arg(args, 2, "node_id")?;
    let direction = direction_arg(args, 3)?;
    let edge_types = edge_type_arg(args, 4)?;
    let limit = usize_arg(args, 5, "limit")?;
    let mut evidence = GraphExecutionEvidence::default();
    let edges = graph_edges(
        env,
        &GraphEdgeRequest {
            graph: &graph,
            node_type: &node_type,
            node_id: &node_id,
            direction: &direction,
            edge_types: &edge_types,
            limit: Some(limit),
        },
        &mut evidence,
    )?;
    let LoadedGraphEdges {
        values: edges,
        _memory: _edge_memory,
    } = edges;
    let mut rows = AccountedVec::try_new(env.controls)?;
    for (index, edge) in edges.into_iter().take(limit).enumerate() {
        let path_bytes = {
            let (next_type, next_id) = adjacent_node_ref(&edge, &node_type, &node_id);
            neighbor_graph_path_bytes(&edge, &node_type, &node_id, next_type, next_id)
        };
        rows.try_push_with(graph_output_variable_bytes(path_bytes), || {
            let (next_type, next_id) = adjacent_node_ref(&edge, &node_type, &node_id);
            GraphPath {
                node_type: next_type.to_owned(),
                node_id: next_id.to_owned(),
                depth: 1,
                cost: edge.weight,
                path_nodes: vec![(node_type.clone(), node_id.clone())],
                path_edges: vec![edge.edge_id.clone()],
                last_edge: Some(edge),
            }
            .into_row(path_rank(index))
        })?;
    }
    let (rows, memory) = rows.into_parts();
    env.cassie
        .runtime
        .record_graph_traversal(&graph.name, "neighbors", 1, rows.len(), "limit");
    evidence.publish(env);
    Ok(GraphTableRows { rows, memory })
}

fn graph_expand(
    env: &source::SourceExecutionEnv<'_>,
    args: &[Value],
) -> Result<GraphTableRows, QueryError> {
    let graph_name = text_arg(args, 0, "graph")?;
    let graph = graph_meta(env, &graph_name)?;
    let start_type = text_arg(args, 1, "node_type")?;
    let start_id = text_arg(args, 2, "node_id")?;
    let max_depth = usize_arg(args, 3, "max_depth")?;
    let direction = direction_arg(args, 4)?;
    let edge_types = edge_type_arg(args, 5)?;
    let max_results = usize_arg(args, 6, "max_results")?;
    let mut evidence = GraphExecutionEvidence::default();
    let (mut queue, mut queue_memory) = initial_graph_queue(env.controls, &start_type, &start_id)?;
    let mut rows = AccountedVec::try_new(env.controls)?;
    let mut expanded_edges = 0usize;

    while let Some(path) = queue.pop_front() {
        let path_bytes = graph_path_bytes(&path);
        check_timeout(env.controls)?;
        if rows.len() >= max_results {
            drop(path);
            release_graph_bytes(&mut queue_memory, path_bytes);
            break;
        }
        if usize::try_from(path.depth).unwrap_or(usize::MAX) >= max_depth {
            drop(path);
            release_graph_bytes(&mut queue_memory, path_bytes);
            continue;
        }
        let edges = graph_edges(
            env,
            &GraphEdgeRequest {
                graph: &graph,
                node_type: &path.node_type,
                node_id: &path.node_id,
                direction: &direction,
                edge_types: &edge_types,
                limit: None,
            },
            &mut evidence,
        )?;
        let LoadedGraphEdges {
            values: edges,
            _memory: _edge_memory,
        } = edges;
        expanded_edges = expanded_edges.saturating_add(edges.len());
        let mut reached_limit = false;
        for edge in edges {
            check_timeout(env.controls)?;
            let (next_type, next_id) = adjacent_node_ref(&edge, &path.node_type, &path.node_id);
            if path
                .path_nodes
                .iter()
                .any(|(node_type, node_id)| node_type == next_type && node_id == next_id)
            {
                continue;
            }
            let next_bytes = next_graph_path_bytes(&path, &edge, next_type, next_id);
            queue_memory.try_grow(next_bytes)?;
            let mut next = path.clone();
            next_type.clone_into(&mut next.node_type);
            next_id.clone_into(&mut next.node_id);
            next.depth += 1;
            next.cost += edge.weight;
            next.path_nodes
                .push((next.node_type.clone(), next.node_id.clone()));
            next.path_edges.push(edge.edge_id.clone());
            next.last_edge = Some(edge);
            debug_assert_eq!(graph_path_bytes(&next), next_bytes);
            let rank = path_rank(rows.len());
            rows.try_push_with(graph_output_variable_bytes(next_bytes), || {
                next.clone().into_row(rank)
            })?;
            if rows.len() >= max_results {
                drop(next);
                release_graph_bytes(&mut queue_memory, next_bytes);
                reached_limit = true;
                break;
            }
            if usize::try_from(next.depth).unwrap_or(usize::MAX) < max_depth {
                try_reserve_graph_slot(|| queue.try_reserve(1))?;
                queue.push_back(next);
            } else {
                drop(next);
                release_graph_bytes(&mut queue_memory, next_bytes);
            }
        }
        drop(path);
        release_graph_bytes(&mut queue_memory, path_bytes);
        if reached_limit {
            break;
        }
    }

    let (rows, memory) = rows.into_parts();
    record_graph_expansion(env, &graph, max_depth, expanded_edges, &rows);
    evidence.publish(env);
    Ok(GraphTableRows { rows, memory })
}

fn record_graph_expansion(
    env: &source::SourceExecutionEnv<'_>,
    graph: &crate::catalog::GraphMeta,
    max_depth: usize,
    expanded_edges: usize,
    rows: &[BatchRow],
) {
    let stop_reason = if expanded_edges == 0 {
        "exhausted"
    } else {
        "limit"
    };
    env.cassie.runtime.record_graph_traversal(
        &graph.name,
        "expand",
        max_depth,
        rows.len(),
        stop_reason,
    );
}

fn graph_shortest_path(
    env: &source::SourceExecutionEnv<'_>,
    args: &[Value],
) -> Result<GraphTableRows, QueryError> {
    let request = shortest_path_request(env, args)?;
    let ShortestPathRequest {
        graph,
        source_type,
        source_id,
        target_type,
        target_id,
        max_depth,
        direction,
        edge_types,
        max_paths,
    } = request;
    let (mut frontier, mut state_memory) =
        initial_graph_frontier(env.controls, source_type, source_id)?;
    let mut best_seen = HashSet::new();
    let mut best_seen_bytes = 0usize;
    let mut found = Vec::new();
    let mut evidence = GraphExecutionEvidence::default();
    let expansion = ShortestExpansion {
        graph: &graph,
        direction: &direction,
        edge_types: &edge_types,
    };

    while !frontier.is_empty() && found.len() < max_paths {
        check_timeout(env.controls)?;
        frontier.sort_by(|left, right| {
            right
                .cost
                .total_cmp(&left.cost)
                .then_with(|| right.node_id.cmp(&left.node_id))
        });
        let Some(path) = frontier.pop() else {
            break;
        };
        let path_bytes = graph_path_bytes(&path);
        let (inserted, is_target) = record_shortest_visit(
            &path,
            &target_type,
            &target_id,
            &mut best_seen,
            &mut best_seen_bytes,
            &mut state_memory,
        )?;
        if !(inserted || is_target) {
            drop(path);
            release_graph_bytes(&mut state_memory, path_bytes);
            continue;
        }
        if is_target && path.depth > 0 {
            try_reserve_graph_slot(|| found.try_reserve(1))?;
            found.push(path);
            continue;
        }
        if usize::try_from(path.depth).unwrap_or(usize::MAX) >= max_depth {
            drop(path);
            release_graph_bytes(&mut state_memory, path_bytes);
            continue;
        }
        extend_shortest_frontier(
            env,
            &expansion,
            &path,
            &mut frontier,
            &mut state_memory,
            &mut evidence,
        )?;
        drop(path);
        release_graph_bytes(&mut state_memory, path_bytes);
    }

    let frontier_bytes = frontier.iter().map(graph_path_bytes).sum();
    drop(frontier);
    release_graph_bytes(&mut state_memory, frontier_bytes);
    drop(best_seen);
    release_graph_bytes(&mut state_memory, best_seen_bytes);
    let mut rows = AccountedVec::try_new(env.controls)?;
    for path in found {
        let path_bytes = graph_path_bytes(&path);
        let rank = path_rank(rows.len());
        rows.try_push_with(graph_output_variable_bytes(path_bytes), || {
            path.into_row(rank)
        })?;
        release_graph_bytes(&mut state_memory, path_bytes);
    }
    debug_assert_eq!(state_memory.bytes(), 0);
    let (rows, memory) = rows.into_parts();
    record_shortest_path(env, &graph, max_depth, &rows);
    evidence.publish(env);
    Ok(GraphTableRows { rows, memory })
}

fn graph_edges(
    env: &source::SourceExecutionEnv<'_>,
    request: &GraphEdgeRequest<'_>,
    evidence: &mut GraphExecutionEvidence,
) -> Result<LoadedGraphEdges, QueryError> {
    let edge_collection = request.graph.edge_collection.as_str();
    let has_overlay = env.session.is_some_and(|session| {
        !session
            .collection_changes_matching(edge_collection)
            .is_empty()
    });
    if has_overlay {
        evidence.fallback_reason = Some("transaction-overlay");
    } else {
        match env.cassie.midge.scan_graph_edges_controlled(
            &GraphEdgeScanRequest {
                graph: request.graph,
                node_type: request.node_type,
                node_id: request.node_id,
                direction: request.direction,
                edge_types: request.edge_types,
                limit: request.limit,
            },
            env.controls,
        )? {
            GraphEdgeScanOutcome::Native {
                edges,
                memory,
                reads,
            } => {
                evidence.reads = evidence.reads.saturating_add(reads);
                evidence.candidates = evidence.candidates.saturating_add(edges.len());
                return Ok(LoadedGraphEdges {
                    values: edges,
                    _memory: memory,
                });
            }
            GraphEdgeScanOutcome::Fallback(reason) => {
                evidence.fallback_reason = Some(reason);
            }
        }
    }

    scan_graph_edges_exact(env, edge_collection, request, evidence)
}

fn scan_graph_edges_exact(
    env: &source::SourceExecutionEnv<'_>,
    edge_collection: &str,
    request: &GraphEdgeRequest<'_>,
    evidence: &mut GraphExecutionEvidence,
) -> Result<LoadedGraphEdges, QueryError> {
    let cursor = env.cassie.open_session_row_cursor(
        env.session,
        edge_collection,
        RowDecode::Full,
        env.controls,
    );
    let mut cursor = match cursor {
        Ok(Some(cursor)) => cursor,
        Ok(None) | Err(crate::app::CassieError::CollectionNotFound(_)) => {
            evidence.fallback_reason = Some("source-collection-missing");
            return Ok(LoadedGraphEdges {
                values: Vec::new(),
                _memory: env.controls.reserve_query_memory(0)?,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let mut edges = AccountedVec::try_new(env.controls)?;
    loop {
        check_timeout(env.controls)?;
        let documents = cursor.next_accounted_documents(&env.cassie.midge, 256, env.controls)?;
        if documents.is_empty() {
            break;
        }
        for document in documents {
            check_timeout(env.controls)?;
            evidence.reads = evidence.reads.saturating_add(1);
            let (document, _document_memory) = document.into_parts();
            let _decode_memory = env
                .controls
                .reserve_query_memory(graph_edge_document_bytes(
                    &document.id,
                    &document.payload,
                    request.graph,
                ))?;
            let Some(edge) = crate::midge::adapter::graph_edge_record_from_payload(
                request.graph,
                &document.id,
                &document.payload,
                true,
            )?
            else {
                continue;
            };
            if graph_edge_matches(
                &edge,
                request.node_type,
                request.node_id,
                request.direction,
                request.edge_types,
            ) {
                edges.try_push_clone(&edge, graph_edge_bytes(&edge))?;
            }
        }
    }
    let (mut edges, memory) = edges.into_parts();
    edges.sort_by(compare_graph_edge_records);
    edges.dedup_by(|right, left| same_executor_graph_edge(left, right));
    if let Some(limit) = request.limit {
        edges.truncate(limit);
    }
    evidence.candidates = evidence.candidates.saturating_add(edges.len());
    Ok(LoadedGraphEdges {
        values: edges,
        _memory: memory,
    })
}

fn graph_edge_matches(
    edge: &GraphEdgeRecord,
    node_type: &str,
    node_id: &str,
    direction: &str,
    edge_types: &[String],
) -> bool {
    let direction_matches = (direction.eq_ignore_ascii_case("out")
        || direction.eq_ignore_ascii_case("both"))
        && edge.source_type.eq_ignore_ascii_case(node_type)
        && edge.source_id == node_id
        || (direction.eq_ignore_ascii_case("in") || direction.eq_ignore_ascii_case("both"))
            && edge.target_type.eq_ignore_ascii_case(node_type)
            && edge.target_id == node_id;
    let type_matches = edge_types.is_empty()
        || edge_types
            .iter()
            .any(|edge_type| edge_type.eq_ignore_ascii_case(&edge.edge_type));
    direction_matches && type_matches
}

fn record_shortest_path(
    env: &source::SourceExecutionEnv<'_>,
    graph: &crate::catalog::GraphMeta,
    max_depth: usize,
    rows: &[BatchRow],
) {
    env.cassie.runtime.record_graph_traversal(
        &graph.name,
        "shortest_path",
        max_depth,
        rows.len(),
        if rows.is_empty() {
            "unreachable"
        } else {
            "target"
        },
    );
}

struct ShortestPathRequest {
    graph: crate::catalog::GraphMeta,
    source_type: String,
    source_id: String,
    target_type: String,
    target_id: String,
    max_depth: usize,
    direction: String,
    edge_types: Vec<String>,
    max_paths: usize,
}

struct ShortestExpansion<'a> {
    graph: &'a crate::catalog::GraphMeta,
    direction: &'a str,
    edge_types: &'a [String],
}

fn extend_shortest_frontier(
    env: &source::SourceExecutionEnv<'_>,
    expansion: &ShortestExpansion<'_>,
    path: &GraphPath,
    frontier: &mut Vec<GraphPath>,
    state_memory: &mut crate::runtime::QueryMemoryReservation,
    evidence: &mut GraphExecutionEvidence,
) -> Result<(), QueryError> {
    let edges = graph_edges(
        env,
        &GraphEdgeRequest {
            graph: expansion.graph,
            node_type: &path.node_type,
            node_id: &path.node_id,
            direction: expansion.direction,
            edge_types: expansion.edge_types,
            limit: None,
        },
        evidence,
    )?;
    let LoadedGraphEdges {
        values: edges,
        _memory: _edge_memory,
    } = edges;
    for edge in edges {
        check_timeout(env.controls)?;
        let (next_type, next_id) = adjacent_node_ref(&edge, &path.node_type, &path.node_id);
        if path
            .path_nodes
            .iter()
            .any(|(node_type, node_id)| node_type == next_type && node_id == next_id)
        {
            continue;
        }
        let next_bytes = next_graph_path_bytes(path, &edge, next_type, next_id);
        state_memory.try_grow(next_bytes)?;
        try_reserve_graph_slot(|| frontier.try_reserve(1))?;
        let mut next = path.clone();
        next_type.clone_into(&mut next.node_type);
        next_id.clone_into(&mut next.node_id);
        next.depth += 1;
        next.cost += edge.weight;
        next.path_nodes
            .push((next.node_type.clone(), next.node_id.clone()));
        next.path_edges.push(edge.edge_id.clone());
        next.last_edge = Some(edge);
        debug_assert_eq!(graph_path_bytes(&next), next_bytes);
        frontier.push(next);
    }
    Ok(())
}

fn shortest_path_request(
    env: &source::SourceExecutionEnv<'_>,
    args: &[Value],
) -> Result<ShortestPathRequest, QueryError> {
    let graph_name = text_arg(args, 0, "graph")?;
    Ok(ShortestPathRequest {
        graph: graph_meta(env, &graph_name)?,
        source_type: text_arg(args, 1, "source_type")?,
        source_id: text_arg(args, 2, "source_id")?,
        target_type: text_arg(args, 3, "target_type")?,
        target_id: text_arg(args, 4, "target_id")?,
        max_depth: usize_arg(args, 5, "max_depth")?,
        direction: direction_arg(args, 6)?,
        edge_types: edge_type_arg(args, 7)?,
        max_paths: usize_arg(args, 8, "max_paths")?,
    })
}

fn graph_path_bytes(path: &GraphPath) -> usize {
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

fn initial_graph_path_bytes(node_type: &str, node_id: &str) -> usize {
    std::mem::size_of::<GraphPath>()
        .saturating_add(node_type.len().saturating_mul(2))
        .saturating_add(node_id.len().saturating_mul(2))
}

fn initial_graph_queue(
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

fn initial_graph_frontier(
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

fn record_shortest_visit(
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

fn neighbor_graph_path_bytes(
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

fn next_graph_path_bytes(
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

fn graph_node_key_bytes(node_type: &str, node_id: &str) -> usize {
    std::mem::size_of::<(String, String)>()
        .saturating_add(node_type.len())
        .saturating_add(node_id.len())
        .saturating_add(std::mem::size_of::<usize>().saturating_mul(2))
}

fn graph_output_variable_bytes(path_bytes: usize) -> usize {
    path_bytes.saturating_mul(2).saturating_add(512)
}

fn release_graph_bytes(memory: &mut crate::runtime::QueryMemoryReservation, released_bytes: usize) {
    let retained_bytes = memory
        .bytes()
        .checked_sub(released_bytes)
        .expect("graph state reservation covers every retained value");
    memory.shrink_to(retained_bytes);
}

fn try_reserve_graph_slot(
    reserve: impl FnOnce() -> Result<(), std::collections::TryReserveError>,
) -> Result<(), QueryError> {
    reserve().map_err(|error| {
        crate::app::CassieError::ResourceLimit(format!(
            "unable to retain controlled graph state: {error}"
        ))
        .into()
    })
}

fn graph_edge_bytes(edge: &crate::midge::adapter::GraphEdgeRecord) -> usize {
    edge.edge_id
        .len()
        .saturating_add(edge.edge_type.len())
        .saturating_add(edge.source_type.len())
        .saturating_add(edge.source_id.len())
        .saturating_add(edge.target_type.len())
        .saturating_add(edge.target_id.len())
        .saturating_add(std::mem::size_of_val(&edge.weight))
}

fn graph_edge_document_bytes(
    id: &str,
    payload: &serde_json::Value,
    graph: &crate::catalog::GraphMeta,
) -> usize {
    std::mem::size_of::<GraphEdgeRecord>()
        .saturating_add(id.len())
        .saturating_add(graph.name.len())
        .saturating_add(json_retained_bytes(payload))
}

fn json_retained_bytes(value: &serde_json::Value) -> usize {
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

fn compare_graph_edge_records(
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

fn same_executor_graph_edge(left: &GraphEdgeRecord, right: &GraphEdgeRecord) -> bool {
    left.graph_id == right.graph_id
        && left.edge_id == right.edge_id
        && left.source_type == right.source_type
        && left.source_id == right.source_id
        && left.target_type == right.target_type
        && left.target_id == right.target_id
        && left.edge_type == right.edge_type
        && left.weight.to_bits() == right.weight.to_bits()
}

fn evaluate_args(
    env: &source::SourceExecutionEnv<'_>,
    function: &FunctionCall,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<Value>, QueryError> {
    let empty = BatchRow::new(Vec::new());
    let row = outer_row.unwrap_or(&empty);
    function
        .args
        .iter()
        .map(|arg| {
            filter::evaluate_expr_value(
                row,
                arg,
                env.params,
                None,
                env.user_functions,
                env.session,
                None,
            )
        })
        .collect()
}

fn graph_meta(
    env: &source::SourceExecutionEnv<'_>,
    graph_name: &str,
) -> Result<crate::catalog::GraphMeta, QueryError> {
    env.cassie
        .catalog
        .get_graph(graph_name)
        .ok_or_else(|| QueryError::General(format!("graph '{graph_name}' does not exist")))
}

fn path_rank(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

fn text_arg(args: &[Value], index: usize, label: &str) -> Result<String, QueryError> {
    match args.get(index) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
        Some(Value::Int64(value)) => Ok(value.to_string()),
        Some(Value::Float64(value)) => Ok(value.to_string()),
        _ => Err(QueryError::General(format!(
            "graph function argument '{label}' must be text"
        ))),
    }
}

fn usize_arg(args: &[Value], index: usize, label: &str) -> Result<usize, QueryError> {
    let value = args
        .get(index)
        .and_then(Value::as_i64)
        .ok_or_else(|| QueryError::General(format!("{label} must be an integer")))?;
    if value < 0 {
        return Err(QueryError::General(format!("{label} must be non-negative")));
    }
    usize::try_from(value).map_err(|_| QueryError::General(format!("{label} is too large")))
}

fn direction_arg(args: &[Value], index: usize) -> Result<String, QueryError> {
    let direction = text_arg(args, index, "direction")?.to_ascii_lowercase();
    if matches!(direction.as_str(), "out" | "in" | "both") {
        Ok(direction)
    } else {
        Err(QueryError::General(
            "graph direction must be 'out', 'in', or 'both'".to_string(),
        ))
    }
}

fn edge_type_arg(args: &[Value], index: usize) -> Result<Vec<String>, QueryError> {
    let raw = text_arg(args, index, "edge_types")?;
    if raw.trim().is_empty() || raw.trim() == "*" {
        return Ok(Vec::new());
    }
    Ok(raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect())
}

fn adjacent_node_ref<'a>(
    edge: &'a crate::midge::adapter::GraphEdgeRecord,
    node_type: &str,
    node_id: &str,
) -> (&'a str, &'a str) {
    if edge.source_type == node_type && edge.source_id == node_id {
        (&edge.target_type, &edge.target_id)
    } else {
        (&edge.source_type, &edge.source_id)
    }
}

impl GraphPath {
    fn into_row(self, path_rank: i64) -> BatchRow {
        let edge = self.last_edge;
        let path_nodes = serde_json::Value::Array(
            self.path_nodes
                .iter()
                .map(|(node_type, node_id)| {
                    serde_json::json!({ "node_type": node_type, "node_id": node_id })
                })
                .collect(),
        );
        let path_edges = serde_json::Value::Array(
            self.path_edges
                .iter()
                .map(|edge_id| serde_json::Value::String(edge_id.clone()))
                .collect(),
        );
        BatchRow::new(vec![
            ("depth".to_string(), Value::Int64(self.depth)),
            ("path_rank".to_string(), Value::Int64(path_rank)),
            ("cost".to_string(), Value::Float64(self.cost)),
            ("node_type".to_string(), Value::String(self.node_type)),
            ("node_id".to_string(), Value::String(self.node_id)),
            (
                "edge_id".to_string(),
                edge.as_ref()
                    .map_or(Value::Null, |edge| Value::String(edge.edge_id.clone())),
            ),
            (
                "edge_type".to_string(),
                edge.as_ref()
                    .map_or(Value::Null, |edge| Value::String(edge.edge_type.clone())),
            ),
            (
                "source_type".to_string(),
                edge.as_ref()
                    .map_or(Value::Null, |edge| Value::String(edge.source_type.clone())),
            ),
            (
                "source_id".to_string(),
                edge.as_ref()
                    .map_or(Value::Null, |edge| Value::String(edge.source_id.clone())),
            ),
            (
                "target_type".to_string(),
                edge.as_ref()
                    .map_or(Value::Null, |edge| Value::String(edge.target_type.clone())),
            ),
            (
                "target_id".to_string(),
                edge.as_ref()
                    .map_or(Value::Null, |edge| Value::String(edge.target_id.clone())),
            ),
            ("path_nodes".to_string(), Value::Json(path_nodes)),
            ("path_edges".to_string(), Value::Json(path_edges)),
        ])
    }
}
