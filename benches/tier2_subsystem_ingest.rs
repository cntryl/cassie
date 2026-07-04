const BENCHMARK: &str = "tier2_subsystem_ingest";
const INGEST_BATCH_SIZE: u64 = 64;

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);
    let write_context = runtime
        .block_on(workloads::context("tier2-ingest", 10_000))
        .expect("benchmark context");
    let mut replay_nonce = 0usize;

    runner.external_timed_batch(
        stress::StressCase::fixed_operations(2, "projection_write_path", "10k"),
        INGEST_BATCH_SIZE,
        || runtime.block_on(workloads::timed_ingest_document_batch(&write_context, 64)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "projection_duplicate_replay", "10k"),
        || {
            replay_nonce = replay_nonce.wrapping_add(1);
            runtime.block_on(workloads::projection_duplicate_replay(
                &write_context,
                replay_nonce,
            ))
        },
    );

    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "projection_lag_catchup", dataset);
        let case =
            stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let context = if dataset == "10k" {
            workloads::replay_context("tier2-ingest-replay-10k", rows)
        } else {
            workloads::replay_context("tier2-ingest-100k", rows)
        };
        let replay_context = runtime.block_on(context).expect("replay benchmark context");
        runner.fixed_operations(case, || {
            replay_nonce = replay_nonce.wrapping_add(1);
            runtime.block_on(workloads::projection_lag_catchup(
                &replay_context,
                replay_nonce,
            ))
        });
    }

    runner.finish();
}
