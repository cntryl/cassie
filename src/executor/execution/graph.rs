use super::{check_timeout, filter, source, BatchRow, FunctionCall, QueryError, Value};
use crate::midge::adapter::{
    GraphEdgeRecord, GraphEdgeScanOutcome, GraphEdgeScanRequest, RowDecode,
};
use crate::runtime::accounted::AccountedVec;

#[path = "graph_support.rs"]
mod graph_support;
use graph_support::{
    compare_graph_edge_records, graph_edge_bytes, graph_edge_document_bytes,
    graph_output_variable_bytes, graph_path_bytes, initial_graph_frontier, initial_graph_queue,
    neighbor_graph_path_bytes, next_graph_path_bytes, reconcile_graph_path_memory,
    release_graph_bytes, release_graph_path_memory, same_executor_graph_edge,
    try_reserve_graph_path_slot, try_reserve_graph_slot, GraphEdgeRequest, GraphExecutionEvidence,
    GraphPath, GraphTableRows, LoadedGraphEdges,
};

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
        rows.try_push_with(graph_output_variable_bytes(path_bytes, 2), || {
            let (next_type, next_id) = adjacent_node_ref(&edge, &node_type, &node_id);
            GraphPath {
                node_type: next_type.to_owned(),
                node_id: next_id.to_owned(),
                depth: 1,
                cost: edge.weight,
                path_nodes: vec![
                    (node_type.clone(), node_id.clone()),
                    (next_type.to_owned(), next_id.to_owned()),
                ],
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
            if path.path_nodes.iter().any(|(node_type, node_id)| {
                node_type.eq_ignore_ascii_case(next_type) && node_id == next_id
            }) {
                continue;
            }
            let estimated_next_bytes = next_graph_path_bytes(&path, &edge, next_type, next_id);
            queue_memory.try_grow(estimated_next_bytes)?;
            let next = path.with_next_edge(edge)?;
            let next_bytes = graph_path_bytes(&next);
            reconcile_graph_path_memory(&mut queue_memory, estimated_next_bytes, next_bytes)?;
            let rank = path_rank(rows.len());
            rows.try_push_with(
                graph_output_variable_bytes(next_bytes, next.path_nodes.len()),
                || next.clone().into_row(rank),
            )?;
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
    let mut found = Vec::new();
    let mut evidence = GraphExecutionEvidence::default();
    let expansion = ShortestExpansion {
        graph: &graph,
        direction: &direction,
        edge_types: &edge_types,
    };

    while !frontier.is_empty() && found.len() < max_paths {
        check_timeout(env.controls)?;
        sort_shortest_frontier(&mut frontier, env.controls)?;
        let Some(path) = frontier.pop() else {
            break;
        };
        let path_bytes = graph_path_bytes(&path);
        let is_target =
            path.node_type.eq_ignore_ascii_case(&target_type) && path.node_id == target_id;
        if is_target && path.depth > 0 {
            try_reserve_graph_path_slot(&mut found, &mut state_memory)?;
            found.push(path);
            continue;
        }
        if usize::try_from(path.depth).unwrap_or(usize::MAX) >= max_depth {
            drop(path);
            release_graph_path_memory(&mut state_memory, path_bytes);
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
        release_graph_path_memory(&mut state_memory, path_bytes);
    }

    let frontier_bytes = frontier
        .iter()
        .map(|path| graph_path_bytes(path).saturating_sub(std::mem::size_of::<GraphPath>()))
        .sum::<usize>()
        .saturating_add(
            frontier
                .capacity()
                .saturating_mul(std::mem::size_of::<GraphPath>()),
        );
    drop(frontier);
    release_graph_bytes(&mut state_memory, frontier_bytes);
    let mut rows = AccountedVec::try_new(env.controls)?;
    let found_slots = found
        .capacity()
        .saturating_mul(std::mem::size_of::<GraphPath>());
    for path in found {
        let path_bytes = graph_path_bytes(&path);
        let rank = path_rank(rows.len());
        rows.try_push_with(
            graph_output_variable_bytes(path_bytes, path.path_nodes.len()),
            || path.into_row(rank),
        )?;
        release_graph_path_memory(&mut state_memory, path_bytes);
    }
    release_graph_bytes(&mut state_memory, found_slots);
    debug_assert_eq!(state_memory.bytes(), 0);
    let (rows, memory) = rows.into_parts();
    record_shortest_path(env, &graph, max_depth, &rows);
    evidence.publish(env);
    Ok(GraphTableRows { rows, memory })
}

fn sort_shortest_frontier(
    frontier: &mut [GraphPath],
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), QueryError> {
    // Rust's stable sort uses insertion sort through twenty elements, then
    // allocates at least forty-eight scratch slots for the general path.
    let scratch_slots = match frontier.len() {
        0..=20 => 0,
        len => len.max(48),
    };
    let _sort_memory = controls
        .reserve_query_memory(scratch_slots.saturating_mul(std::mem::size_of::<GraphPath>()))?;
    frontier.sort_by(|left, right| {
        right
            .cost
            .total_cmp(&left.cost)
            .then_with(|| right.node_id.cmp(&left.node_id))
    });
    check_timeout(controls)
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
        if path.path_nodes.iter().any(|(node_type, node_id)| {
            node_type.eq_ignore_ascii_case(next_type) && node_id == next_id
        }) {
            continue;
        }
        let estimated_next_bytes = next_graph_path_bytes(path, &edge, next_type, next_id)
            .saturating_sub(std::mem::size_of::<GraphPath>());
        state_memory.try_grow(estimated_next_bytes)?;
        try_reserve_graph_path_slot(frontier, state_memory)?;
        let next = path.with_next_edge(edge)?;
        let next_bytes = graph_path_bytes(&next).saturating_sub(std::mem::size_of::<GraphPath>());
        reconcile_graph_path_memory(state_memory, estimated_next_bytes, next_bytes)?;
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
    let context = env.cassie.binding_context_for_session(env.session);
    let graph_name = crate::sql::binder::normalize_relation_name(graph_name, &context)
        .map_err(|error| QueryError::General(error.to_string()))?;
    env.cassie
        .catalog
        .get_graph_exact(&graph_name)
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
    if edge.source_type.eq_ignore_ascii_case(node_type) && edge.source_id == node_id {
        (&edge.target_type, &edge.target_id)
    } else {
        (&edge.source_type, &edge.source_id)
    }
}

impl GraphPath {
    fn with_next_edge(&self, edge: GraphEdgeRecord) -> Result<Self, QueryError> {
        let (next_type, next_id) = adjacent_node_ref(&edge, &self.node_type, &self.node_id);
        let node_type = next_type.to_owned();
        let node_id = next_id.to_owned();
        let mut path_nodes = Vec::new();
        try_reserve_graph_slot(|| {
            path_nodes.try_reserve_exact(self.path_nodes.len().saturating_add(1))
        })?;
        path_nodes.extend(self.path_nodes.iter().cloned());
        path_nodes.push((node_type.clone(), node_id.clone()));
        let mut path_edges = Vec::new();
        try_reserve_graph_slot(|| {
            path_edges.try_reserve_exact(self.path_edges.len().saturating_add(1))
        })?;
        path_edges.extend(self.path_edges.iter().cloned());
        path_edges.push(edge.edge_id.clone());
        Ok(Self {
            node_type,
            node_id,
            depth: self.depth + 1,
            cost: self.cost + edge.weight,
            path_nodes,
            path_edges,
            last_edge: Some(edge),
        })
    }

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

#[cfg(test)]
mod frontier_tests {
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

    use super::{sort_shortest_frontier, GraphPath};

    #[test]
    fn should_budget_minimum_frontier_sort_scratch_allocation() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 21 * std::mem::size_of::<GraphPath>(),
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut frontier = (0..21)
            .map(|cost| GraphPath {
                node_type: "n".to_string(),
                node_id: "s".to_string(),
                depth: 1,
                cost: f64::from(cost),
                path_nodes: Vec::new(),
                path_edges: Vec::new(),
                last_edge: None,
            })
            .collect::<Vec<_>>();

        // Act
        let result = sort_shortest_frontier(&mut frontier, &controls);

        // Assert
        assert!(matches!(
            result,
            Err(super::QueryError::Cassie(
                crate::app::CassieError::ResourceLimit(_)
            ))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_reject_frontier_sort_when_scratch_exceeds_query_budget() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 1_024 * std::mem::size_of::<GraphPath>(),
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let path = |cost| GraphPath {
            node_type: "n".to_string(),
            node_id: "s".to_string(),
            depth: 1,
            cost,
            path_nodes: Vec::new(),
            path_edges: Vec::new(),
            last_edge: None,
        };
        let mut frontier = (0..1_025)
            .map(|cost| path(f64::from(cost)))
            .collect::<Vec<_>>();

        // Act
        let result = sort_shortest_frontier(&mut frontier, &controls);

        // Assert
        assert!(matches!(
            result,
            Err(super::QueryError::Cassie(
                crate::app::CassieError::ResourceLimit(_)
            ))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_recheck_cancellation_after_sorting_the_shortest_path_frontier() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation,
        );
        let path = |cost, node_id: &str| GraphPath {
            node_type: "n".to_string(),
            node_id: node_id.to_string(),
            depth: 1,
            cost,
            path_nodes: vec![("n".to_string(), node_id.to_string())],
            path_edges: Vec::new(),
            last_edge: None,
        };
        let mut frontier = vec![path(1.0, "earlier"), path(2.0, "later")];

        // Act
        let result = sort_shortest_frontier(&mut frontier, &controls);

        // Assert
        assert!(result.is_err(), "cancelled query must fail after sorting");
        assert_eq!(
            frontier.iter().map(|path| path.cost).collect::<Vec<_>>(),
            vec![2.0, 1.0],
            "the post-sort timeout check must run after frontier ordering"
        );
    }
}
