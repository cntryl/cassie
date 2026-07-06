#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const COLD_START_BATCH: u64 = 3;
const WARM_QUERY_BATCH: u64 = 8;

fn main() {
    let runtime = workloads::runtime();
    let warm_ctx = runtime
        .block_on(workloads::context("tier3-warm-start", 10_000))
        .expect("benchmark context");

    let mut runner = stress::runner("tier3_system_startup");
    runner.fixed_batch(
        stress::StressCase::fixed_operations(3, "cold_start", "10k"),
        COLD_START_BATCH,
        || {
            for index in 0..COLD_START_BATCH {
                let label = format!("tier3-cold-start-{index}");
                runtime
                    .block_on(workloads::empty_context(&label))
                    .expect("cold start context");
            }
        },
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(3, "warm_start_query", "10k"),
        WARM_QUERY_BATCH,
        || {
            for _ in 0..WARM_QUERY_BATCH {
                runtime.block_on(workloads::execute_sql(
                    &warm_ctx,
                    "SELECT count(*) FROM bench_documents",
                ));
            }
        },
    );
    runner.finish();
}
