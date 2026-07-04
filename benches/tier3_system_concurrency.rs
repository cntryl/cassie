#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-concurrency", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier3_system_concurrency");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, "concurrent_queries", "8x10k")
            .parameter("client_count", "8"),
        || runtime.block_on(workloads::concurrent_queries(&ctx, 8)),
    );
    runner.finish();
}
