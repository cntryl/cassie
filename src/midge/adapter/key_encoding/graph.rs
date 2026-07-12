use super::{
    data_scoped_key, data_scoped_prefix, prefix, scoped_key, FAMILY_GRAPH, FAMILY_GRAPH_ADJACENCY,
};

pub(crate) fn graph_prefix() -> Vec<u8> {
    prefix(FAMILY_GRAPH, &[])
}

pub(crate) fn graph_key(name: &str) -> Vec<u8> {
    let normalized = name.trim().to_ascii_lowercase();
    scoped_key(FAMILY_GRAPH, &normalized, &[])
}

pub(crate) fn graph_adjacency_prefix(graph: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_GRAPH_ADJACENCY, graph, &[])
}

pub(crate) fn graph_outbound_prefix(graph: &str, source_type: &str, source_id: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_GRAPH_ADJACENCY,
        graph,
        &[b"out", source_type.as_bytes(), source_id.as_bytes()],
    )
}

pub(crate) fn graph_inbound_prefix(graph: &str, target_type: &str, target_id: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_GRAPH_ADJACENCY,
        graph,
        &[b"in", target_type.as_bytes(), target_id.as_bytes()],
    )
}

pub(crate) fn graph_outbound_edge_key(
    graph: &str,
    source_type: &str,
    source_id: &str,
    edge_type: &str,
    target_type: &str,
    target_id: &str,
    edge_id: &str,
) -> Vec<u8> {
    data_scoped_key(
        FAMILY_GRAPH_ADJACENCY,
        graph,
        &[
            b"out",
            source_type.as_bytes(),
            source_id.as_bytes(),
            edge_type.as_bytes(),
            target_type.as_bytes(),
            target_id.as_bytes(),
            edge_id.as_bytes(),
        ],
    )
}

pub(crate) fn graph_inbound_edge_key(
    graph: &str,
    target_type: &str,
    target_id: &str,
    edge_type: &str,
    source_type: &str,
    source_id: &str,
    edge_id: &str,
) -> Vec<u8> {
    data_scoped_key(
        FAMILY_GRAPH_ADJACENCY,
        graph,
        &[
            b"in",
            target_type.as_bytes(),
            target_id.as_bytes(),
            edge_type.as_bytes(),
            source_type.as_bytes(),
            source_id.as_bytes(),
            edge_id.as_bytes(),
        ],
    )
}
