//! Exact topology-axis fixtures; generic Tier 5 row sizes are unchanged.

pub(super) fn is_graph_topology_fixture(
    owner: &str,
    workload: &str,
    scale: &str,
    rows: usize,
    scaling: bool,
) -> bool {
    scaling
        && owner == "tier5_scaling_graph"
        && matches!(
            (workload, scale, rows),
            ("graph_degree_scaling_query", "degree-16", 33)
                | ("graph_degree_scaling_query", "degree-64", 129)
                | ("graph_degree_scaling_query", "degree-256", 513)
                | ("graph_depth_scaling_query", "depth-2", 5)
                | ("graph_depth_scaling_query", "depth-4", 9)
                | ("graph_depth_scaling_query", "depth-8", 17)
                | ("graph_fanout_scaling_query", "fanout-2", 29)
                | ("graph_fanout_scaling_query", "fanout-4", 169)
                | ("graph_fanout_scaling_query", "fanout-8", 1169)
        )
}
