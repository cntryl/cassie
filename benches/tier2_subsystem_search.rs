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
    let small_context = runtime
        .block_on(workloads::context("tier2-search", 10_000))
        .expect("benchmark context");
    let large_context = runtime
        .block_on(workloads::context("tier2-search-100k", 100_000))
        .expect("100k benchmark context");

    let mut group = c.benchmark_group("tier2_subsystem_search");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    let small_case =
        performance_benchmarks::expect_benchmark(BENCHMARK, "full_text_executor", "10k");
    group.bench_function(
        BenchmarkId::new(small_case.workload, small_case.fixture_scale),
        |b| {
            b.iter(|| {
                runtime.block_on(workloads::execute_sql(
                    &small_context,
                    "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
                ))
            });
        },
    );
    let large_case =
        performance_benchmarks::expect_benchmark(BENCHMARK, "full_text_executor", "100k");
    group.bench_function(
        BenchmarkId::new(large_case.workload, large_case.fixture_scale),
        |b| {
            b.iter(|| {
                runtime.block_on(workloads::execute_sql(
                    &large_context,
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
