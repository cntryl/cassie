use std::collections::{HashSet, VecDeque};

use super::{source, FunctionCall, BatchRow, QueryError, Value, filter};

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

pub(super) fn execute_table_function(
    env: &source::SourceExecutionEnv<'_>,
    function: &FunctionCall,
    outer_row: Option<&BatchRow>,
) -> Result<Vec<BatchRow>, QueryError> {
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
) -> Result<Vec<BatchRow>, QueryError> {
    let graph = graph_meta(env, text_arg(args, 0, "graph")?)?;
    let node_type = text_arg(args, 1, "node_type")?;
    let node_id = text_arg(args, 2, "node_id")?;
    let direction = direction_arg(args, 3)?;
    let edge_types = edge_type_arg(args, 4)?;
    let limit = usize_arg(args, 5, "limit")?;
    let edges = env
        .cassie
        .midge
        .scan_graph_edges(&graph, &node_type, &node_id, &direction, &edge_types)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let rows: Vec<BatchRow> = edges
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(index, edge)| {
            let (next_type, next_id) = adjacent_node(&edge, &node_type, &node_id);
            GraphPath {
                node_type: next_type,
                node_id: next_id,
                depth: 1,
                cost: edge.weight,
                path_nodes: vec![(node_type.clone(), node_id.clone())],
                path_edges: vec![edge.edge_id.clone()],
                last_edge: Some(edge),
            }
            .into_row(index as i64)
        })
        .collect();
    env.cassie
        .runtime
        .record_graph_traversal(&graph.name, "neighbors", 1, rows.len(), "limit");
    Ok(rows)
}

fn graph_expand(
    env: &source::SourceExecutionEnv<'_>,
    args: &[Value],
) -> Result<Vec<BatchRow>, QueryError> {
    let graph = graph_meta(env, text_arg(args, 0, "graph")?)?;
    let start_type = text_arg(args, 1, "node_type")?;
    let start_id = text_arg(args, 2, "node_id")?;
    let max_depth = usize_arg(args, 3, "max_depth")?;
    let direction = direction_arg(args, 4)?;
    let edge_types = edge_type_arg(args, 5)?;
    let max_results = usize_arg(args, 6, "max_results")?;
    let mut queue = VecDeque::from([GraphPath {
        node_type: start_type.clone(),
        node_id: start_id.clone(),
        depth: 0,
        cost: 0.0,
        path_nodes: vec![(start_type.clone(), start_id.clone())],
        path_edges: Vec::new(),
        last_edge: None,
    }]);
    let mut rows = Vec::new();
    let mut expanded_edges = 0usize;

    while let Some(path) = queue.pop_front() {
        if rows.len() >= max_results {
            break;
        }
        if usize::try_from(path.depth).unwrap_or(usize::MAX) >= max_depth {
            continue;
        }
        let edges = env
            .cassie
            .midge
            .scan_graph_edges(
                &graph,
                &path.node_type,
                &path.node_id,
                &direction,
                &edge_types,
            )
            .map_err(|error| QueryError::General(error.to_string()))?;
        expanded_edges = expanded_edges.saturating_add(edges.len());
        for edge in edges {
            let (next_type, next_id) = adjacent_node(&edge, &path.node_type, &path.node_id);
            if path
                .path_nodes
                .iter()
                .any(|(node_type, node_id)| node_type == &next_type && node_id == &next_id)
            {
                continue;
            }
            let mut next = path.clone();
            next.node_type = next_type;
            next.node_id = next_id;
            next.depth += 1;
            next.cost += edge.weight;
            next.path_nodes
                .push((next.node_type.clone(), next.node_id.clone()));
            next.path_edges.push(edge.edge_id.clone());
            next.last_edge = Some(edge);
            rows.push(next.clone().into_row(rows.len() as i64));
            if rows.len() >= max_results {
                break;
            }
            queue.push_back(next);
        }
    }

    env.cassie.runtime.record_graph_traversal(
        &graph.name,
        "expand",
        max_depth,
        rows.len(),
        if expanded_edges == 0 {
            "exhausted"
        } else {
            "limit"
        },
    );
    Ok(rows)
}

fn graph_shortest_path(
    env: &source::SourceExecutionEnv<'_>,
    args: &[Value],
) -> Result<Vec<BatchRow>, QueryError> {
    let graph = graph_meta(env, text_arg(args, 0, "graph")?)?;
    let source_type = text_arg(args, 1, "source_type")?;
    let source_id = text_arg(args, 2, "source_id")?;
    let target_type = text_arg(args, 3, "target_type")?;
    let target_id = text_arg(args, 4, "target_id")?;
    let max_depth = usize_arg(args, 5, "max_depth")?;
    let direction = direction_arg(args, 6)?;
    let edge_types = edge_type_arg(args, 7)?;
    let max_paths = usize_arg(args, 8, "max_paths")?;
    let mut frontier = vec![GraphPath {
        node_type: source_type.clone(),
        node_id: source_id.clone(),
        depth: 0,
        cost: 0.0,
        path_nodes: vec![(source_type, source_id)],
        path_edges: Vec::new(),
        last_edge: None,
    }];
    let mut best_seen = HashSet::new();
    let mut found = Vec::new();

    while !frontier.is_empty() && found.len() < max_paths {
        frontier.sort_by(|left, right| {
            right
                .cost
                .total_cmp(&left.cost)
                .then_with(|| right.node_id.cmp(&left.node_id))
        });
        let Some(path) = frontier.pop() else {
            break;
        };
        if !(best_seen.insert(format!("{}:{}", path.node_type, path.node_id))
            || path.node_type == target_type && path.node_id == target_id)
        {
            continue;
        }
        if path.node_type == target_type && path.node_id == target_id && path.depth > 0 {
            found.push(path);
            continue;
        }
        if usize::try_from(path.depth).unwrap_or(usize::MAX) >= max_depth {
            continue;
        }
        let edges = env
            .cassie
            .midge
            .scan_graph_edges(
                &graph,
                &path.node_type,
                &path.node_id,
                &direction,
                &edge_types,
            )
            .map_err(|error| QueryError::General(error.to_string()))?;
        for edge in edges {
            let (next_type, next_id) = adjacent_node(&edge, &path.node_type, &path.node_id);
            if path
                .path_nodes
                .iter()
                .any(|(node_type, node_id)| node_type == &next_type && node_id == &next_id)
            {
                continue;
            }
            let mut next = path.clone();
            next.node_type = next_type;
            next.node_id = next_id;
            next.depth += 1;
            next.cost += edge.weight;
            next.path_nodes
                .push((next.node_type.clone(), next.node_id.clone()));
            next.path_edges.push(edge.edge_id.clone());
            next.last_edge = Some(edge);
            frontier.push(next);
        }
    }

    let rows = found
        .into_iter()
        .enumerate()
        .map(|(index, path)| path.into_row(index as i64))
        .collect::<Vec<_>>();
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
    Ok(rows)
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
    graph_name: String,
) -> Result<crate::catalog::GraphMeta, QueryError> {
    env.cassie
        .catalog
        .get_graph(&graph_name)
        .ok_or_else(|| QueryError::General(format!("graph '{graph_name}' does not exist")))
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

fn adjacent_node(
    edge: &crate::midge::adapter::GraphEdgeRecord,
    node_type: &str,
    node_id: &str,
) -> (String, String) {
    if edge.source_type == node_type && edge.source_id == node_id {
        (edge.target_type.clone(), edge.target_id.clone())
    } else {
        (edge.source_type.clone(), edge.source_id.clone())
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
