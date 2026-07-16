use super::super::graphs::GraphEdgeRecord;
use super::{encoded_u64_component, key, prefix, scoped_key, FAMILY_GRAPH, FAMILY_GRAPH_ADJACENCY};

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

pub(crate) fn graph_outbound_prefix(graph_id: u64, source_type: &str, source_id: &str) -> Vec<u8> {
    let graph = encoded_u64_component(graph_id);
    prefix(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            b"O",
            source_type.as_bytes(),
            source_id.as_bytes(),
        ],
    )
}

pub(crate) fn graph_inbound_prefix(graph_id: u64, target_type: &str, target_id: &str) -> Vec<u8> {
    let graph = encoded_u64_component(graph_id);
    prefix(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            b"I",
            target_type.as_bytes(),
            target_id.as_bytes(),
        ],
    )
}

pub(crate) fn graph_outbound_edge_key(record: &GraphEdgeRecord) -> Vec<u8> {
    let graph = encoded_u64_component(record.graph_id);
    let weight = sortable_weight(record.weight);
    key(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            b"O",
            record.source_type.as_bytes(),
            record.source_id.as_bytes(),
            record.edge_type.as_bytes(),
            weight.as_bytes(),
            record.target_type.as_bytes(),
            record.target_id.as_bytes(),
            record.edge_id.as_bytes(),
        ],
    )
}

pub(crate) fn graph_inbound_edge_key(record: &GraphEdgeRecord) -> Vec<u8> {
    let graph = encoded_u64_component(record.graph_id);
    let weight = sortable_weight(record.weight);
    key(
        FAMILY_GRAPH_ADJACENCY,
        &[
            graph.as_slice(),
            b"I",
            record.target_type.as_bytes(),
            record.target_id.as_bytes(),
            record.edge_type.as_bytes(),
            weight.as_bytes(),
            record.source_type.as_bytes(),
            record.source_id.as_bytes(),
            record.edge_id.as_bytes(),
        ],
    )
}

fn sortable_weight(value: f64) -> String {
    let bits = value.to_bits();
    let ordered = if bits & (1_u64 << 63) == 0 {
        bits ^ (1_u64 << 63)
    } else {
        !bits
    };
    format!("{ordered:016x}")
}
