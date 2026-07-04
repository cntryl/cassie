#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier3-mixed-load", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier3_system_mixed_load");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, "mixed_ingest_query", "10k"),
        || runtime.block_on(workloads::mixed_ingest_query(&ctx)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, "large_result_set", "512_rows"),
        || runtime.block_on(workloads::large_result_set_query(&ctx)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(3, "ten_million_row_query_shape", "scaled"),
        || runtime.block_on(workloads::ten_million_row_query_shape(&ctx)),
    );
    runner.finish();
}
