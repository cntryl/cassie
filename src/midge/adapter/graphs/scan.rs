use cntryl_midge::Query;

use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryExecutionControls;

use super::{
    CassieError, GraphAdjacencyManifest, GraphEdgeRecord, GraphEdgeScanOutcome,
    GraphEdgeScanRequest, Midge, GRAPH_ADJACENCY_FORMAT_VERSION,
};

const GRAPH_SCAN_PAGE_ENTRIES: usize = 128;

struct GraphPrefixScan<'a> {
    graph: &'a crate::catalog::GraphMeta,
    prefix: Vec<u8>,
    direction: &'static str,
    node_type: &'a str,
    node_id: &'a str,
    limit: Option<usize>,
}

impl Midge {
    pub(crate) fn scan_graph_edges_controlled(
        &self,
        request: &GraphEdgeScanRequest<'_>,
        controls: &QueryExecutionControls,
    ) -> Result<GraphEdgeScanOutcome, CassieError> {
        check_controls(controls)?;
        let graph = self.resolve_graph_storage(request.graph)?;
        let edge_collection = self.canonical_collection_name(&graph.edge_collection);
        let tx = self.begin_data_readonly_tx_for(&edge_collection)?;
        let Some(manifest) = load_manifest(&tx, graph.storage_id)? else {
            return Ok(GraphEdgeScanOutcome::Fallback("missing-sidecar-manifest"));
        };
        if manifest.format_version != GRAPH_ADJACENCY_FORMAT_VERSION {
            return Ok(GraphEdgeScanOutcome::Fallback("sidecar-format-mismatch"));
        }
        let snapshot_generation = collection_generation_from_tx(&tx, &edge_collection)?;
        if manifest.source_generation != snapshot_generation {
            return Ok(GraphEdgeScanOutcome::Fallback(
                "sidecar-generation-mismatch",
            ));
        }

        let mut edges = AccountedVec::try_new(controls)?;
        let mut reads = 0usize;
        if request.limit != Some(0) {
            for prefix_request in graph_prefix_scans(
                &graph,
                request.node_type,
                request.node_id,
                request.direction,
                request.edge_types,
                request.limit,
            ) {
                match self.scan_graph_prefix_controlled(
                    &tx,
                    &prefix_request,
                    controls,
                    &mut edges,
                    &mut reads,
                ) {
                    Ok(()) => {}
                    Err(CassieError::Parse(_)) => {
                        return Ok(GraphEdgeScanOutcome::Fallback("malformed-sidecar"));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        check_controls(controls)?;
        drop(tx);
        if self.collection_generation(&edge_collection)? != snapshot_generation {
            return Ok(GraphEdgeScanOutcome::Fallback("concurrent-source-change"));
        }

        let (mut edges, memory) = edges.into_parts();
        edges.sort_by(compare_graph_edges);
        edges.dedup_by(|right, left| same_graph_edge(left, right));
        if let Some(limit) = request.limit {
            edges.truncate(limit);
        }
        Ok(GraphEdgeScanOutcome::Native {
            edges,
            memory,
            reads,
        })
    }

    fn resolve_graph_storage(
        &self,
        graph: &crate::catalog::GraphMeta,
    ) -> Result<crate::catalog::GraphMeta, CassieError> {
        if graph.storage_id != 0 {
            return Ok(graph.clone());
        }
        self.list_graphs()?
            .into_iter()
            .find(|stored| crate::catalog::name_matches(&stored.name, &graph.name))
            .ok_or_else(|| CassieError::Parse(format!("graph '{}' not found", graph.name)))
    }

    fn scan_graph_prefix_controlled(
        &self,
        tx: &cntryl_midge::Transaction,
        request: &GraphPrefixScan<'_>,
        controls: &QueryExecutionControls,
        edges: &mut AccountedVec<GraphEdgeRecord>,
        reads: &mut usize,
    ) -> Result<(), CassieError> {
        let mut last_key: Option<Vec<u8>> = None;
        let mut remaining = request.limit.unwrap_or(usize::MAX);
        while remaining > 0 {
            check_controls(controls)?;
            let page_limit = remaining.min(GRAPH_SCAN_PAGE_ENTRIES);
            let mut query = Query::new()
                .prefix(request.prefix.clone().into())
                .limit(page_limit);
            if let Some(last_key) = last_key.as_ref() {
                let mut next_key = last_key.clone();
                next_key.push(0);
                query = query.start_key(next_key.into());
            }
            let mut scan = tx.scan(&query).map_err(CassieError::from)?;
            let mut page_entries = 0usize;
            while page_entries < page_limit {
                check_controls(controls)?;
                let Some(entry) = scan.next() else {
                    break;
                };
                let (key, value) = entry.map_err(CassieError::from)?;
                self.record_query_scan_entry();
                if super::super::query_scan_control::should_cancel_controlled_query_scan() {
                    return Err(CassieError::QueryCancelled);
                }
                if !value.is_empty() {
                    return Err(CassieError::Parse(
                        "graph adjacency values must be empty".to_owned(),
                    ));
                }
                let variable_bytes = graph_edge_variable_bytes(&key)?;
                edges.try_push_with_result(variable_bytes, || {
                    decode_graph_edge_key(request, &key)
                })?;
                last_key = Some(key.to_vec());
                page_entries = page_entries.saturating_add(1);
                *reads = reads.saturating_add(1);
                remaining = remaining.saturating_sub(1);
            }
            if page_entries < page_limit {
                break;
            }
        }
        Ok(())
    }
}

fn graph_prefix_scans<'a>(
    graph: &'a crate::catalog::GraphMeta,
    node_type: &'a str,
    node_id: &'a str,
    direction: &str,
    edge_types: &[String],
    limit: Option<usize>,
) -> Vec<GraphPrefixScan<'a>> {
    let mut scans = Vec::new();
    let outbound = direction.eq_ignore_ascii_case("out") || direction.eq_ignore_ascii_case("both");
    let inbound = direction.eq_ignore_ascii_case("in") || direction.eq_ignore_ascii_case("both");
    if edge_types.is_empty() {
        if outbound {
            scans.push(GraphPrefixScan {
                graph,
                prefix: super::super::key_encoding::graph_outbound_prefix(
                    graph.storage_id,
                    node_type,
                    node_id,
                ),
                direction: "out",
                node_type,
                node_id,
                limit,
            });
        }
        if inbound {
            scans.push(GraphPrefixScan {
                graph,
                prefix: super::super::key_encoding::graph_inbound_prefix(
                    graph.storage_id,
                    node_type,
                    node_id,
                ),
                direction: "in",
                node_type,
                node_id,
                limit,
            });
        }
        return scans;
    }

    for (index, edge_type) in edge_types.iter().enumerate() {
        if edge_types[..index]
            .iter()
            .any(|prior| prior.eq_ignore_ascii_case(edge_type))
        {
            continue;
        }
        if outbound {
            scans.push(GraphPrefixScan {
                graph,
                prefix: super::super::key_encoding::graph_outbound_edge_type_prefix(
                    graph.storage_id,
                    node_type,
                    node_id,
                    edge_type,
                ),
                direction: "out",
                node_type,
                node_id,
                limit,
            });
        }
        if inbound {
            scans.push(GraphPrefixScan {
                graph,
                prefix: super::super::key_encoding::graph_inbound_edge_type_prefix(
                    graph.storage_id,
                    node_type,
                    node_id,
                    edge_type,
                ),
                direction: "in",
                node_type,
                node_id,
                limit,
            });
        }
    }
    scans
}

fn decode_graph_edge_key(
    request: &GraphPrefixScan<'_>,
    key: &[u8],
) -> Result<GraphEdgeRecord, CassieError> {
    let suffix = key
        .strip_prefix(request.prefix.as_slice())
        .ok_or_else(|| CassieError::Parse("invalid graph adjacency prefix".to_owned()))?;
    let components = suffix
        .split(|byte| *byte == 0)
        .filter(|component| !component.is_empty())
        .map(std::str::from_utf8)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| CassieError::Parse(format!("invalid graph adjacency key: {error}")))?;
    let [weight, edge_id, edge_type, other_type, other_id] = components.as_slice() else {
        return Err(CassieError::Parse(
            "invalid graph adjacency component count".to_owned(),
        ));
    };
    let weight = decode_sortable_weight(weight)?;
    let (source_type, source_id, target_type, target_id) = if request.direction == "out" {
        (request.node_type, request.node_id, *other_type, *other_id)
    } else {
        (*other_type, *other_id, request.node_type, request.node_id)
    };
    Ok(GraphEdgeRecord {
        graph: request.graph.name.clone(),
        graph_id: request.graph.storage_id,
        edge_id: (*edge_id).to_owned(),
        source_type: source_type.to_owned(),
        source_id: source_id.to_owned(),
        target_type: target_type.to_owned(),
        target_id: target_id.to_owned(),
        edge_type: (*edge_type).to_owned(),
        weight,
    })
}

fn load_manifest(
    tx: &cntryl_midge::Transaction,
    graph_id: u64,
) -> Result<Option<GraphAdjacencyManifest>, CassieError> {
    let Some(raw) = tx
        .get(&super::super::key_encoding::graph_manifest_key(graph_id))
        .map_err(CassieError::from)?
    else {
        return Ok(None);
    };
    Ok(serde_json::from_slice(&raw).ok())
}

fn collection_generation_from_tx(
    tx: &cntryl_midge::Transaction,
    collection: &str,
) -> Result<u64, CassieError> {
    let Some(raw) = tx
        .get(&super::super::key_encoding::collection_generation_key(
            collection,
        ))
        .map_err(CassieError::from)?
    else {
        return Ok(0);
    };
    let bytes: [u8; 8] = raw
        .as_ref()
        .try_into()
        .map_err(|_| CassieError::Parse("invalid collection generation".to_owned()))?;
    Ok(u64::from_be_bytes(bytes))
}

fn decode_sortable_weight(value: &str) -> Result<f64, CassieError> {
    let ordered = u64::from_str_radix(value, 16)
        .map_err(|error| CassieError::Parse(format!("invalid graph weight: {error}")))?;
    let bits = if ordered & (1_u64 << 63) == 0 {
        !ordered
    } else {
        ordered ^ (1_u64 << 63)
    };
    Ok(f64::from_bits(bits))
}

fn graph_edge_variable_bytes(key: &[u8]) -> Result<usize, CassieError> {
    key.len().checked_mul(2).ok_or_else(|| {
        CassieError::ResourceLimit("graph edge retained-size accounting overflow".to_owned())
    })
}

fn compare_graph_edges(left: &GraphEdgeRecord, right: &GraphEdgeRecord) -> std::cmp::Ordering {
    left.weight
        .total_cmp(&right.weight)
        .then_with(|| left.edge_id.cmp(&right.edge_id))
        .then_with(|| left.source_type.cmp(&right.source_type))
        .then_with(|| left.source_id.cmp(&right.source_id))
        .then_with(|| left.target_type.cmp(&right.target_type))
        .then_with(|| left.target_id.cmp(&right.target_id))
}

fn same_graph_edge(left: &GraphEdgeRecord, right: &GraphEdgeRecord) -> bool {
    left.graph_id == right.graph_id
        && left.edge_id == right.edge_id
        && left.source_type == right.source_type
        && left.source_id == right.source_id
        && left.target_type == right.target_type
        && left.target_id == right.target_id
        && left.edge_type == right.edge_type
        && left.weight.to_bits() == right.weight.to_bits()
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
