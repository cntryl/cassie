use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_search(c: &mut Criterion) {
    const BENCHMARK: &str = "tier2_subsystem_search";

    let runtime = workloads::runtime();
    let ctx_10k = runtime
        .block_on(workloads::context("tier2-search", 10_000))
        .expect("benchmark context");
    let ctx_100k = runtime
        .block_on(workloads::context("tier2-search-100k", 100_000))
        .expect("100k benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_search");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    let benchmark_10k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "full_text_executor", "10k");
    group.bench_function(
        BenchmarkId::new(benchmark_10k.workload, benchmark_10k.fixture_scale),
        |b| {
            b.iter(|| {
                runtime.block_on(workloads::execute_sql(
                    &ctx_10k,
                    "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
                ))
            });
        },
    );
    let benchmark_100k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "full_text_executor", "100k");
    group.bench_function(
        BenchmarkId::new(benchmark_100k.workload, benchmark_100k.fixture_scale),
        |b| {
        b.iter(|| {
            runtime.block_on(workloads::execute_sql(
                &ctx_100k,
                "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
            ))
        });
        },
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier2();
    targets = bench_search
}

criterion_main!(benches);
