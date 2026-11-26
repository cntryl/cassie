//! The narrow topology exception used within Tier 5's Batch validation.

use cassie_bench_support::stress::FixtureIdentityTracker;

#[path = "../../../cassie-bench-support/src/performance_graph_topology.rs"]
mod topology;

#[test]
fn should_register_all_topology_fixtures_without_reusing_an_owner_scale_identity() {
    // Arrange
    let scenarios = crate::performance_benchmarks::benchmark_scenarios()
        .filter(|scenario| scenario.benchmark == "tier5_scaling_graph")
        .collect::<Vec<_>>();
    let mut identities = FixtureIdentityTracker::default();

    // Act
    let outcomes = scenarios
        .iter()
        .map(|scenario| {
            identities.register(
                scenario.benchmark,
                scenario.fixture_scale,
                &format!(
                    "{}/{}/{}",
                    scenario.benchmark, scenario.workload, scenario.fixture_scale
                ),
            )
        })
        .collect::<Vec<_>>();
    let changed_identity = identities.register("tier5_scaling_graph", "depth-2", "another-fixture");

    // Assert
    assert_eq!(outcomes.len(), 9);
    assert!(outcomes.iter().all(Result::is_ok));
    assert!(changed_identity.is_err());
}

#[test]
fn should_reject_legacy_bare_scales_for_graph_topology_fixtures() {
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
    let admitted = points.map(|(workload, scale, rows)| {
        topology::is_graph_topology_fixture("tier5_scaling_graph", workload, scale, rows, true)
    });

    // Assert
    assert_eq!(admitted, [false; 9]);
}

#[test]
fn should_admit_only_the_declared_graph_topology_points() {
    // Arrange
    let points = [
        ("graph_degree_scaling_query", "degree-16", 33),
        ("graph_degree_scaling_query", "degree-64", 129),
        ("graph_degree_scaling_query", "degree-256", 513),
        ("graph_depth_scaling_query", "depth-2", 5),
        ("graph_depth_scaling_query", "depth-4", 9),
        ("graph_depth_scaling_query", "depth-8", 17),
        ("graph_fanout_scaling_query", "fanout-2", 29),
        ("graph_fanout_scaling_query", "fanout-4", 169),
        ("graph_fanout_scaling_query", "fanout-8", 1169),
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
            "depth-4",
            9,
            true,
        ),
        ("tier5_scaling_graph", "unknown", "depth-4", 9, true),
        (
            "tier5_scaling_graph",
            "graph_depth_scaling_query",
            "depth-04",
            9,
            true,
        ),
        (
            "tier5_scaling_graph",
            "graph_depth_scaling_query",
            "depth-4",
            9,
            false,
        ),
        (
            "tier5_scaling_graph",
            "graph_depth_scaling_query",
            "depth-4",
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

#[test]
fn should_resolve_a_memory_profile_for_every_registered_topology_scenario() {
    // Arrange
    let scenarios = crate::performance_benchmarks::benchmark_scenarios()
        .filter(|scenario| scenario.benchmark == "tier5_scaling_graph")
        .collect::<Vec<_>>();
    // Act
    let profiles = scenarios
        .iter()
        .map(|scenario| {
            crate::performance_benchmarks::validate_scenario_contract(scenario).unwrap();
            crate::performance_benchmarks::deployment_profile_for_scenario(scenario).unwrap()
        })
        .collect::<Vec<_>>();
    // Assert
    assert_eq!(scenarios.len(), 9);
    for profile in profiles {
        assert_eq!(profile.profile_id, "local-dev-graph-topology-memory-64m");
        assert_eq!(profile.storage_mode, "in_memory_midge_fallback");
        assert_eq!(profile.fixture_scale, "graph-topology-axes");
    }
}

#[test]
fn should_reject_invalid_topology_catalog_contracts_without_relabeling_other_scales() {
    // Arrange
    use crate::performance_benchmarks::{BenchmarkTier, BenchmarkTimingMode, FixtureClass};
    let baseline = *crate::performance_benchmarks::benchmark_for_benchmark(
        "tier5_scaling_graph",
        "graph_depth_scaling_query",
        "depth-4",
    )
    .unwrap();
    let invalid = [
        crate::performance_benchmarks::PerformanceBenchmarkScenario {
            benchmark: "tier5_scaling_query",
            ..baseline
        },
        crate::performance_benchmarks::PerformanceBenchmarkScenario {
            fixture_class: FixtureClass::Representative,
            ..baseline
        },
        crate::performance_benchmarks::PerformanceBenchmarkScenario {
            fixture_rows: 10,
            ..baseline
        },
        crate::performance_benchmarks::PerformanceBenchmarkScenario {
            timing_mode: BenchmarkTimingMode::Micro,
            ..baseline
        },
        crate::performance_benchmarks::PerformanceBenchmarkScenario {
            declared_tier: BenchmarkTier::Tier4,
            ..baseline
        },
    ];
    // Act
    let contracts = invalid.map(|scenario| {
        crate::performance_benchmarks::validate_scenario_contract(&scenario).is_ok()
    });
    let profiles = invalid
        .map(|scenario| crate::performance_benchmarks::deployment_profile_for_scenario(&scenario));
    // Assert
    assert_eq!(contracts, [false; 5]);
    assert!(profiles.iter().all(Option::is_none));
    for scenario in crate::performance_benchmarks::benchmark_scenarios().filter(|scenario| {
        scenario.declared_tier == crate::performance_benchmarks::BenchmarkTier::Tier5
            && scenario.benchmark != "tier5_scaling_graph"
    }) {
        assert!([10_000, 100_000, 250_000].contains(&scenario.fixture_rows));
        assert_ne!(
            crate::performance_benchmarks::deployment_profile_for_scenario(scenario)
                .unwrap()
                .profile_id,
            "local-dev-graph-topology-memory-64m"
        );
    }
}

#[path = "../../benches/support/weighted_graph_product_status.rs"]
mod weighted_graph_product_status;

#[test]
fn should_require_valid_weighted_route_measurements_for_the_target() {
    // Arrange
    let outcomes = vec![serde_json::json!({"matches_oracle": true, "fallback": false}); 100];

    // Act
    let actual = [
        weighted_graph_product_status::measurement_status("full", 50.0, false, &outcomes),
        weighted_graph_product_status::measurement_status("full", 50.0, true, &outcomes),
        weighted_graph_product_status::measurement_status("full", 101.0, true, &outcomes),
        weighted_graph_product_status::measurement_status("smoke", 50.0, true, &outcomes),
    ];

    // Assert
    assert_eq!(
        actual,
        [
            (false, Some(false)),
            (true, Some(true)),
            (true, Some(false)),
            (true, None)
        ]
    );
}
