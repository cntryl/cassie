#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let warm_ctx = runtime
        .block_on(workloads::context("tier3-warm-start", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier3_system_startup");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, "cold_start", "10k"),
        || runtime.block_on(workloads::empty_context("tier3-cold-start")),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, "warm_start_query", "10k"),
        || {
            runtime.block_on(workloads::execute_sql(
                &warm_ctx,
                "SELECT count(*) FROM bench_documents",
            ))
        },
    );
    runner.finish();
}
