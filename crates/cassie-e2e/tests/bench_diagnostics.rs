#[path = "bench_diagnostics/relations/semantics.rs"]
mod semantics;

#[path = "bench_diagnostics/replay_mode.rs"]
mod replay_mode;

#[path = "bench_diagnostics/replay_read_ownership.rs"]
mod replay_read_ownership;

#[path = "bench_diagnostics/replay_index_metadata.rs"]
mod replay_index_metadata;

#[path = "bench_diagnostics/graph_topology.rs"]
mod graph_topology;

#[path = "../benches/tier3_system_storage_modes/profiles.rs"]
mod column_profiles;

#[path = "../benches/support/performance_benchmarks.rs"]
pub mod performance_benchmarks;

#[path = "../benches/support/product_mixed.rs"]
pub mod product_mixed;

#[test]
fn should_preserve_local_column_product_profile() {
    // Arrange
    let profile = column_profiles::Profile::named("100k_256m_local");

    // Act
    let qualifies = profile.is_advantage_profile();

    // Assert
    assert!(qualifies);
    assert!(profile.local);
    assert_eq!(profile.fact_rows, 100_000);
    assert_eq!(profile.query_budget, 256 * 1024 * 1024);
}
