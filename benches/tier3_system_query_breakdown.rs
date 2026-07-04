#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-query-breakdown", 10_000))
        .expect("benchmark context");

    let breakdown = (0..12)
        .map(|_| runtime.block_on(workloads::simple_10k_query_breakdown(&ctx)))
        .min_by_key(|breakdown| breakdown.total)
        .expect("query breakdown sample");
    println!(
        "{}",
        serde_json::to_string_pretty(&breakdown).expect("serialize query breakdown")
    );

    let mut runner = stress::runner("tier3_system_query_breakdown");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, "simple_10k", "breakdown"),
        || runtime.block_on(workloads::simple_10k_query_breakdown(&ctx)),
    );
    runner.finish();
}
