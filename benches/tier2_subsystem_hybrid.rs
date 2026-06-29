use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_hybrid(c: &mut Criterion) {
    const BENCHMARK: &str = "tier2_subsystem_hybrid";

    let runtime = workloads::runtime();
    let ctx_10k = runtime
        .block_on(workloads::context("tier2-hybrid", 10_000))
        .expect("benchmark context");
    let ctx_100k = runtime
        .block_on(workloads::context("tier2-hybrid-100k", 100_000))
        .expect("100k benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_hybrid");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    for (dataset, ctx) in [("10k", &ctx_10k), ("100k", &ctx_100k)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "hybrid_executor", dataset);
        group.bench_function(
            BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
            |b| {
                b.iter(|| {
                    runtime.block_on(workloads::execute_sql(
                        ctx,
                        "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 20",
                    ))
                });
            },
        );
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_hybrid
}

criterion_main!(benches);
