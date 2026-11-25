//! Check all nine topology rows with the same summarizer used by benchmark reporting.

#[path = "../../cassie-e2e/benches/support/performance_benchmarks.rs"]
pub mod performance_benchmarks;

fn main() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    assert_eq!(
        arguments.len(),
        2,
        "usage: summarize_graph_scaling <artifact.json>"
    );
    let artifact = std::fs::read_to_string(&arguments[1]).expect("read graph artifact");
    let metadata: serde_json::Value = serde_json::from_str(&artifact).expect("artifact JSON");
    assert_eq!(
        metadata["metadata"]["deployment_profile_id"].as_str(),
        Some("local-dev-graph-topology-memory-64m"),
        "artifact must explicitly declare the topology profile"
    );
    let rows = performance_benchmarks::summarize_stress_artifact_rows(&artifact)
        .expect("artifact row identities");
    assert_eq!(
        rows.len(),
        9,
        "artifact must contain nine scenario summaries"
    );
    let scenarios = performance_benchmarks::benchmark_scenarios()
        .filter(|scenario| scenario.benchmark == "tier5_scaling_graph")
        .collect::<Vec<_>>();
    assert_eq!(
        scenarios.len(),
        9,
        "registered graph topology scenario count"
    );
    let mut summaries = Vec::new();
    for scenario in scenarios {
        performance_benchmarks::validate_scenario_contract(scenario)
            .expect("registered scenario contract");
        let summary = performance_benchmarks::summarize_stress_artifact(scenario, &artifact)
            .expect("graph artifact scenario summary");
        assert_eq!(
            summary.profile_id, "local-dev-graph-topology-memory-64m",
            "actual artifact must name the topology memory profile"
        );
        assert_eq!(summary.storage_mode, "in_memory_midge_fallback");
        summaries.push(serde_json::json!({
            "scenario": summary.scenario_id,
            "profile": summary.profile_id,
            "declared_profile_storage": summary.storage_mode,
            "scale": summary.fixture_scale,
            "p50_us": summary.p50_us,
            "p95_us": summary.p95_us,
            "p99_us": summary.p99_us,
            "throughput_ops_per_sec": summary.throughput_ops_per_sec,
        }));
    }
    println!(
        "{}",
        serde_json::json!({
            "evidence": "local diagnostic; no release or Stable promotion claim",
            "summaries": summaries,
        })
    );
}
