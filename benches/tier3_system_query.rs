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
            "mixed_order_scalar_query",
            "10k",
            "SELECT id FROM bench_documents WHERE status = 'approved' AND score >= 10 ORDER BY status DESC, score ASC LIMIT 50",
        ),
        (
            "expression_index_query",
            "10k",
            "SELECT id FROM bench_documents WHERE lower(title) = 'title-1' LIMIT 50",
        ),
        (
            "expression_index_range_query",
            "10k",
            "SELECT id FROM bench_documents WHERE lower(title) >= 'title-4' AND lower(title) < 'title-9' LIMIT 50",
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
    let runnable_cases = cases
        .into_iter()
        .filter(|(name, dataset, _)| should_run_case(name, dataset))
        .collect::<Vec<_>>();

    if !runnable_cases.is_empty() {
        let ctx_10k = runtime
            .block_on(workloads::context("tier3-query", 10_000))
            .expect("benchmark context");
        for (name, dataset, sql) in runnable_cases {
            if matches!(
                name,
                "simple_sql_query"
                    | "mixed_order_scalar_query"
                    | "expression_index_query"
                    | "expression_index_range_query"
            ) {
                performance_benchmarks::expect_benchmark(BENCHMARK, name, dataset);
            }
            let _ = runtime.block_on(workloads::execute_sql(&ctx_10k, sql));
            group.bench_function(BenchmarkId::new(name, dataset), |b| {
                b.iter(|| runtime.block_on(workloads::execute_sql(&ctx_10k, sql)))
            });
        }
    }

    if should_run_case("simple_sql_query", "100k") {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "simple_sql_query", "100k");
        let ctx_100k = runtime
            .block_on(workloads::unindexed_context("tier3-query-100k", 100_000))
            .expect("100k benchmark context");
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
    }

    let scalar_100k_cases = [
        (
            "mixed_order_scalar_query",
            "SELECT id FROM bench_documents WHERE status = 'approved' AND score >= 10 ORDER BY status DESC, score ASC LIMIT 50",
        ),
        (
            "expression_index_query",
            "SELECT id FROM bench_documents WHERE lower(title) = 'title-1' LIMIT 50",
        ),
        (
            "expression_index_range_query",
            "SELECT id FROM bench_documents WHERE lower(title) >= 'title-4' AND lower(title) < 'title-9' LIMIT 50",
        ),
    ];
    let runnable_scalar_100k_cases = scalar_100k_cases
        .into_iter()
        .filter(|(workload, _)| should_run_case(workload, "100k"))
        .collect::<Vec<_>>();
    if !runnable_scalar_100k_cases.is_empty() {
        let scalar_ctx_100k = runtime
            .block_on(workloads::scalar_context(
                "tier3-query-scalar-100k",
                100_000,
            ))
            .expect("100k scalar benchmark context");
        for (workload, sql) in runnable_scalar_100k_cases {
            let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, workload, "100k");
            let _ = runtime.block_on(workloads::execute_sql(&scalar_ctx_100k, sql));
            group.bench_function(
                BenchmarkId::new(benchmark.workload, benchmark.fixture_scale),
                |b| b.iter(|| runtime.block_on(workloads::execute_sql(&scalar_ctx_100k, sql))),
            );
        }
    }

    if should_run_case("time_series_window_scan", "10k") {
        let time_series_ctx_10k = runtime
            .block_on(workloads::time_series_context("tier3-query-ts", 10_000))
            .expect("time-series benchmark context");
        let ts_10k =
            performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_window_scan", "10k");
        group.bench_function(
            BenchmarkId::new(ts_10k.workload, ts_10k.fixture_scale),
            |b| {
                b.iter(|| {
                    runtime.block_on(workloads::time_series_window_scan(&time_series_ctx_10k))
                })
            },
        );
    }

    if should_run_case("graph_expand_query", "10k") {
        let graph_ctx_10k = runtime
            .block_on(workloads::graph_context("tier3-query-graph", 10_000))
            .expect("graph benchmark context");
        let graph_10k =
            performance_benchmarks::expect_benchmark(BENCHMARK, "graph_expand_query", "10k");
        group.bench_function(
            BenchmarkId::new(graph_10k.workload, graph_10k.fixture_scale),
            |b| {
                b.iter(|| {
                    runtime.block_on(workloads::execute_sql(
                        &graph_ctx_10k,
                        "SELECT node_id FROM graph_expand('bench_graph', 'doc', 'node-0', 4, 'out', 'links', 64)",
                    ))
                })
            },
        );
    }

    if should_run_case("graph_expand_query", "100k") {
        let graph_ctx_100k = runtime
            .block_on(workloads::graph_context("tier3-query-graph-100k", 100_000))
            .expect("100k graph benchmark context");
        let graph_100k =
            performance_benchmarks::expect_benchmark(BENCHMARK, "graph_expand_query", "100k");
        group.bench_function(
            BenchmarkId::new(graph_100k.workload, graph_100k.fixture_scale),
            |b| {
                b.iter(|| {
                    runtime.block_on(workloads::execute_sql(
                        &graph_ctx_100k,
                        "SELECT node_id FROM graph_expand('bench_graph', 'doc', 'node-0', 4, 'out', 'links', 64)",
                    ))
                })
            },
        );
    }

    if should_run_case("time_series_window_scan", "100k") {
        let time_series_ctx_100k = runtime
            .block_on(workloads::time_series_context(
                "tier3-query-ts-100k",
                100_000,
            ))
            .expect("100k time-series benchmark context");
        let ts_100k =
            performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_window_scan", "100k");
        group.bench_function(
            BenchmarkId::new(ts_100k.workload, ts_100k.fixture_scale),
            |b| {
                b.iter(|| {
                    runtime.block_on(workloads::time_series_window_scan(&time_series_ctx_100k))
                })
            },
        );
    }

    group.finish();
}

fn should_run_case(workload: &str, fixture_scale: &str) -> bool {
    let filters = std::env::args()
        .skip(1)
        .filter(|arg| arg != "--bench")
        .filter(|arg| !arg.starts_with("--"))
        .collect::<Vec<_>>();
    if filters.is_empty() {
        return true;
    }

    let local_id = format!("{workload}/{fixture_scale}");
    let full_id = format!("tier3_system_query/{local_id}");
    filters
        .iter()
        .any(|filter| full_id.contains(filter) || local_id.contains(filter))
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier3();
    targets = bench_query
}

criterion_main!(benches);
