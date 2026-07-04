const BENCHMARK: &str = "tier2_subsystem_hybrid";

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
            performance_benchmarks::expect_benchmark(BENCHMARK, "hybrid_executor", dataset);
        let case =
            stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let context = runtime
            .block_on(workloads::context(&format!("tier2-hybrid-{dataset}"), rows))
            .expect("benchmark context");
        runner.fixed_operations(case, || {
            runtime.block_on(workloads::execute_sql(
                &context,
                "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 20",
            ))
        });
    }

    runner.finish();
}
