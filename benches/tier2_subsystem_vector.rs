const BENCHMARK: &str = "tier2_subsystem_vector";

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "vector_executor", dataset);
        let case =
            stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let context = runtime
            .block_on(workloads::unindexed_context(
                &format!("tier2-vector-{dataset}"),
                rows,
            ))
            .expect("benchmark context");
        runner.fixed_operations(case, || {
            runtime.block_on(workloads::execute_sql(
                &context,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20",
            ))
        });
    }

    runner.finish();
}
