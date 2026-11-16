//! The narrow topology exception used within Tier 5's Batch validation.

#[path = "../../benches/support/performance_graph_topology.rs"]
mod topology;

#[test]
fn should_admit_only_the_declared_graph_topology_points() {
    // Arrange
    let points = [
        ("graph_degree_scaling_query", "16", 33),
        ("graph_degree_scaling_query", "64", 129),
        ("graph_degree_scaling_query", "256", 513),
        ("graph_depth_scaling_query", "2", 5),
        ("graph_depth_scaling_query", "4", 9),
        ("graph_depth_scaling_query", "8", 17),
        ("graph_fanout_scaling_query", "2", 29),
        ("graph_fanout_scaling_query", "4", 169),
        ("graph_fanout_scaling_query", "8", 1169),
    ];
    // Act
    let actual = points.map(|(workload, scale, rows)| {
        topology::is_graph_topology_fixture("tier5_scaling_graph", workload, scale, rows, true)
    });
    // Assert
    assert_eq!(actual, [true; 9]);
    for (workload, scale, rows) in points {
        assert!(!topology::is_graph_topology_fixture(
            "tier5_scaling_graph",
            workload,
            scale,
            rows - 1,
            true
        ));
        assert!(!topology::is_graph_topology_fixture(
            "tier5_scaling_graph",
            workload,
            scale,
            rows + 1,
            true
        ));
    }
}

#[test]
fn should_reject_changes_to_the_topology_owner_workload_scale_or_class() {
    // Arrange
    let invalid = [
        (
            "tier5_scaling_query",
            "graph_depth_scaling_query",
            "4",
            9,
            true,
        ),
        ("tier5_scaling_graph", "unknown", "4", 9, true),
        (
            "tier5_scaling_graph",
            "graph_depth_scaling_query",
            "04",
            9,
            true,
        ),
        (
            "tier5_scaling_graph",
            "graph_depth_scaling_query",
            "4",
            9,
            false,
        ),
        (
            "tier5_scaling_graph",
            "graph_depth_scaling_query",
            "4",
            10_000,
            true,
        ),
    ];
    // Act
    let actual = invalid.map(|(owner, workload, scale, rows, scaling)| {
        topology::is_graph_topology_fixture(owner, workload, scale, rows, scaling)
    });
    // Assert
    assert_eq!(actual, [false; 5]);
}
