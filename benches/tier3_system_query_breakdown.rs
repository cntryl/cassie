#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const BREAKDOWN_BATCH: u64 = 12;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-query-breakdown", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier3_system_query_breakdown");
    runner.fixed_batch(
        stress::StressCase::fixed_operations(3, "simple_10k", "breakdown"),
        BREAKDOWN_BATCH,
        || {
            let best = (0..BREAKDOWN_BATCH)
                .map(|_| runtime.block_on(workloads::simple_10k_query_breakdown(&ctx)))
                .min_by_key(|breakdown| breakdown.total)
                .expect("query breakdown sample");
            std::hint::black_box(best);
        },
    );
    runner.finish();
}
