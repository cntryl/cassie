#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-plan-cache", 1_024))
        .expect("benchmark context");
    runtime.block_on(workloads::plan_cache_hit(&ctx));
    let mut miss_nonce = 0usize;

    let mut runner = stress::runner("tier2_subsystem_plan_cache");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "plan_cache_hit", "10k"),
        || runtime.block_on(workloads::plan_cache_hit(&ctx)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "plan_cache_miss", "10k"),
        || {
            miss_nonce = miss_nonce.wrapping_add(1);
            runtime.block_on(workloads::plan_cache_miss(&ctx, miss_nonce))
        },
    );
    runner.finish();
}
