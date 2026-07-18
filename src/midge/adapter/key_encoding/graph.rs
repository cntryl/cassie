use super::super::graphs::GraphEdgeRecord;
use super::{encoded_u64_component, key, prefix, scoped_key, FAMILY_GRAPH, FAMILY_GRAPH_ADJACENCY};

const OUTBOUND_EDGE_TYPE: &[u8] = b"OE";
const INBOUND_EDGE_TYPE: &[u8] = b"IE";
const OUTBOUND_WEIGHT: &[u8] = b"OW";
const INBOUND_WEIGHT: &[u8] = b"IW";
const MANIFEST: &[u8] = b"M";

pub(crate) fn graph_prefix() -> Vec<u8> {
    prefix(FAMILY_GRAPH, &[])
}

pub(crate) fn graph_key(name: &str) -> Vec<u8> {
    let normalized = name.trim().to_ascii_lowercase();
    scoped_key(FAMILY_GRAPH, &normalized, &[])
}

pub(crate) fn graph_adjacency_prefix(graph_id: u64) -> Vec<u8> {
    let graph = encoded_u64_component(graph_id);
    prefix(FAMILY_GRAPH_ADJACENCY, &[graph.as_slice()])
}

pub(crate) fn graph_manifest_key(graph_id: u64) -> Vec<u8> {
    let graph = encoded_u64_component(graph_id);
    key(FAMILY_GRAPH_ADJACENCY, &[graph.as_slice(), MANIFEST])
}

pub(crate) fn graph_outbound_prefix(graph_id: u64, source_type: &str, source_id: &str) -> Vec<u8> {
    graph_node_prefix(graph_id, OUTBOUND_WEIGHT, source_type, source_id)
}

pub(crate) fn graph_inbound_prefix(graph_id: u64, target_type: &str, target_id: &str) -> Vec<u8> {
    graph_node_prefix(graph_id, INBOUND_WEIGHT, target_type, target_id)
}

pub(crate) fn graph_outbound_edge_type_prefix(
    graph_id: u64,
    source_type: &str,
    source_id: &str,
    edge_type: &str,
) -> Vec<u8> {
    graph_node_type_prefix(
        graph_id,
        OUTBOUND_EDGE_TYPE,
        source_type,
        source_id,
        edge_type,
    )
}

pub(crate) fn graph_inbound_edge_type_prefix(
    graph_id: u64,
    target_type: &str,
    target_id: &str,
    edge_type: &str,
) -> Vec<u8> {
    graph_node_type_prefix(
        graph_id,
        INBOUND_EDGE_TYPE,
        target_type,
        target_id,
        edge_type,
    )
}

pub(crate) fn graph_outbound_edge_key(record: &GraphEdgeRecord) -> Vec<u8> {
    graph_weight_edge_key(record, OUTBOUND_WEIGHT, true)
}

pub(crate) fn graph_inbound_edge_key(record: &GraphEdgeRecord) -> Vec<u8> {
    graph_weight_edge_key(record, INBOUND_WEIGHT, false)
}

pub(crate) fn graph_outbound_edge_type_key(record: &GraphEdgeRecord) -> Vec<u8> {
    graph_type_edge_key(record, OUTBOUND_EDGE_TYPE, true)
}

pub(crate) fn graph_inbound_edge_type_key(record: &GraphEdgeRecord) -> Vec<u8> {
    graph_type_edge_key(record, INBOUND_EDGE_TYPE, false)
}

fn graph_node_prefix(graph_id: u64, ordering: &[u8], node_type: &str, node_id: &str) -> Vec<u8> {
    let graph = encoded_u64_component(graph_id);
    prefix(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            ordering,
            node_type.as_bytes(),
            node_id.as_bytes(),
        ],
    )
}

fn graph_node_type_prefix(
    graph_id: u64,
    ordering: &[u8],
    node_type: &str,
    node_id: &str,
    edge_type: &str,
) -> Vec<u8> {
    let edge_type = edge_type.to_ascii_lowercase();
    let graph = encoded_u64_component(graph_id);
    prefix(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            ordering,
            node_type.as_bytes(),
            node_id.as_bytes(),
            edge_type.as_bytes(),
        ],
    )
}

fn graph_weight_edge_key(record: &GraphEdgeRecord, ordering: &[u8], outbound: bool) -> Vec<u8> {
    let graph = encoded_u64_component(record.graph_id);
    let weight = sortable_weight(record.weight);
    let (node_type, node_id, other_type, other_id) = graph_edge_nodes(record, outbound);
    key(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            ordering,
            node_type,
            node_id,
            weight.as_bytes(),
            record.edge_id.as_bytes(),
            record.edge_type.as_bytes(),
            other_type,
            other_id,
        ],
    )
}

fn graph_type_edge_key(record: &GraphEdgeRecord, ordering: &[u8], outbound: bool) -> Vec<u8> {
    let graph = encoded_u64_component(record.graph_id);
    let weight = sortable_weight(record.weight);
    let normalized_edge_type = record.edge_type.to_ascii_lowercase();
    let (node_type, node_id, other_type, other_id) = graph_edge_nodes(record, outbound);
    key(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            ordering,
            node_type,
            node_id,
            normalized_edge_type.as_bytes(),
            weight.as_bytes(),
            record.edge_id.as_bytes(),
            record.edge_type.as_bytes(),
            other_type,
            other_id,
        ],
    )
}

fn graph_edge_nodes(record: &GraphEdgeRecord, outbound: bool) -> (&[u8], &[u8], &[u8], &[u8]) {
    if outbound {
        (
            record.source_type.as_bytes(),
            record.source_id.as_bytes(),
            record.target_type.as_bytes(),
            record.target_id.as_bytes(),
        )
    } else {
        (
            record.target_type.as_bytes(),
            record.target_id.as_bytes(),
            record.source_type.as_bytes(),
            record.source_id.as_bytes(),
        )
    }
}

pub(crate) fn sortable_weight(value: f64) -> String {
    let bits = value.to_bits();
    let ordered = if bits & (1_u64 << 63) == 0 {
        bits ^ (1_u64 << 63)
    } else {
        !bits
    };
    format!("{ordered:016x}")
}
