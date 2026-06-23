use criterion::{
    criterion_group, criterion_main, BenchmarkId, Criterion, SamplingMode, Throughput,
};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_query(c: &mut Criterion) {
    const BENCHMARK: &str = "tier3_system_query";

    let runtime = workloads::runtime();
    let ctx_10k = runtime
        .block_on(workloads::context("tier3-query", 10_000))
        .expect("benchmark context");
    let ctx_100k = runtime
        .block_on(workloads::unindexed_context("tier3-query-100k", 100_000))
        .expect("100k benchmark context");
    let time_series_ctx_10k = runtime
        .block_on(workloads::time_series_context("tier3-query-ts", 10_000))
        .expect("time-series benchmark context");
    let time_series_ctx_100k = runtime
        .block_on(workloads::time_series_context(
            "tier3-query-ts-100k",
            100_000,
        ))
        .expect("100k time-series benchmark context");

    let mut group = c.benchmark_group("tier3_system_query");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    let cases = [
        (
            "simple_sql_query",
            "10k",
            "SELECT id, title FROM bench_documents WHERE id = 'doc-1'",
        ),
        (
            "indexed_filter_query",
            "10k",
            "SELECT id FROM bench_documents WHERE score = 1",
        ),
        (
            "range_query",
            "10k",
            "SELECT id FROM bench_documents WHERE score >= 10 LIMIT 100",
        ),
        (
            "sort_limit_query",
            "10k",
            "SELECT id FROM bench_documents ORDER BY score DESC LIMIT 50",
        ),
        (
            "fulltext_search_query",
            "10k",
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') ORDER BY score DESC LIMIT 20",
        ),
        (
            "vector_search_query",
            "10k",
            "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20",
        ),
        (
            "hybrid_search_query",
            "10k",
            "SELECT id, hybrid_score(search_score(body, 'alpha'), vector_score(embedding, '[1,0,0]')) AS score FROM bench_documents ORDER BY score DESC LIMIT 20",
        ),
    ];

    for (name, dataset, sql) in cases {
        if name == "simple_sql_query" {
            performance_benchmarks::expect_benchmark(BENCHMARK, name, dataset);
        }
        group.bench_function(BenchmarkId::new(name, dataset), |b| {
            b.iter(|| runtime.block_on(workloads::execute_sql(&ctx_10k, sql)))
        });
    }
    let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, "simple_sql_query", "100k");
    group.bench_function(
        BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
        |b| {
            b.iter(|| {
                runtime.block_on(workloads::execute_sql(
                    &ctx_100k,
                    "SELECT id, title FROM bench_documents WHERE id = 'doc-1'",
                ))
            })
        },
    );
    let ts_10k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_window_scan", "10k");
    group.bench_function(
        BenchmarkId::new(ts_10k.workload, ts_10k.fixture_scale),
        |b| b.iter(|| runtime.block_on(workloads::time_series_window_scan(&time_series_ctx_10k))),
    );
    let ts_100k =
        performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_window_scan", "100k");
    group.bench_function(
        BenchmarkId::new(ts_100k.workload, ts_100k.fixture_scale),
        |b| b.iter(|| runtime.block_on(workloads::time_series_window_scan(&time_series_ctx_100k))),
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_query
}

criterion_main!(benches);
