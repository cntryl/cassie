//! Exact topology-axis fixtures; generic Tier 5 row sizes are unchanged.

pub(crate) fn is_graph_topology_fixture(
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
            ("graph_degree_scaling_query", "16", 33)
                | ("graph_degree_scaling_query", "64", 129)
                | ("graph_degree_scaling_query", "256", 513)
                | ("graph_depth_scaling_query", "2", 5)
                | ("graph_depth_scaling_query", "4", 9)
                | ("graph_depth_scaling_query", "8", 17)
                | ("graph_fanout_scaling_query", "2", 29)
                | ("graph_fanout_scaling_query", "4", 169)
                | ("graph_fanout_scaling_query", "8", 1169)
        )
}
